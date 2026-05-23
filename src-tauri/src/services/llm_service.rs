//! LLM Service — Multi-protocol LLM client with SSE streaming
//!
//! Supports OpenAI (Chat Completions) and Anthropic (Messages) protocols.
//! The user selects a provider in settings; the backend uses that provider's
//! native protocol directly — no protocol conversion needed.
//!
//! Provides the full RAG pipeline:
//!   embed query → hybrid search → context assembly → LLM completion (SSE)
//!
//! Graceful fallback: when LLM is unavailable, returns search results only.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::hybrid_search::{self, HybridSearchResult};
use crate::services::metadata::MetadataStore;
use crate::services::vector_index::VectorIndex;

// ─── Constants ───

/// System prompt — immutable ERP consultant knowledge assistant (from SPEC.md §5.6)
const SYSTEM_PROMPT: &str = "\
你是一个金蝶ERP实施顾问知识助手。你的知识库来自用户本地的文档。\n\
当知识库中有相关信息时，请基于这些信息回答。\n\
当知识库中没有相关信息时，请明确说明\"知识库中暂无相关内容\"，\n\
不要编造答案。\n\
\n\
回答时请标注来源，例如：（来源：星达铜业项目深度案例.md）";

/// Default context window size in tokens
const DEFAULT_MAX_CONTEXT_TOKENS: u32 = 4096;

/// Tokens reserved for the assistant's response
const RESPONSE_TOKENS: u32 = 1024;

/// Default OpenAI base URL
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// Default OpenAI model
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o";

/// Default Anthropic base URL
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";

/// Default Anthropic model
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-3-5-sonnet-20241022";

/// Anthropic API version header
const ANTHROPIC_VERSION: &str = "2023-06-01";

// ─── Provider ───

/// LLM provider type — determines which API protocol to use
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LLMProvider {
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "anthropic")]
    Anthropic,
}

// ─── Configuration ───

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    /// Which provider to use (determines API protocol)
    pub provider: LLMProvider,
    /// API key for authentication
    pub api_key: String,
    /// Base URL (default varies by provider)
    pub base_url: String,
    /// Model name (default varies by provider)
    pub model: String,
    /// Max context window in tokens (default: 4096)
    pub max_tokens: u32,
    /// Temperature for generation (default: 0.3)
    pub temperature: f32,
}

impl Default for LLMConfig {
    fn default() -> Self {
        Self {
            provider: LLMProvider::OpenAI,
            api_key: String::new(),
            base_url: DEFAULT_OPENAI_BASE_URL.to_string(),
            model: DEFAULT_OPENAI_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_CONTEXT_TOKENS,
            temperature: 0.3,
        }
    }
}

// ─── Chat Message ───

/// A chat message for the conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// ─── SSE Event ───

/// A single SSE streaming chunk from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// The delta text content (may be empty for intermediate chunks)
    pub content: String,
    /// Whether this is the final chunk
    pub done: bool,
}

// ─── RAG Response (non-streaming fallback) ───

/// Full RAG response with sources (used for fallback mode)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGResponse {
    /// The AI-generated answer
    pub answer: String,
    /// Source chunks used for context
    pub sources: Vec<RAGSource>,
    /// Whether LLM was available
    pub llm_available: bool,
}

/// A source reference in the RAG response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGSource {
    pub title: String,
    pub section_path: Option<String>,
    pub content_snippet: String,
    pub score: f32,
}

// ─── Token Counting ───

/// Count tokens in a string using tiktoken-rs (cl100k_base encoding).
///
/// Falls back to a rough char-based estimate if tiktoken fails.
pub fn count_tokens(text: &str) -> u32 {
    match tiktoken_rs::cl100k_base() {
        Ok(bpe) => bpe.encode_with_special_tokens(text).len() as u32,
        Err(_) => {
            // Rough fallback: ~4 chars per token for mixed CJK/English
            (text.chars().count() as f32 / 2.5).ceil() as u32
        }
    }
}

/// Truncate text to fit within a token budget.
///
/// Preserves UTF-8 character boundaries by truncating at the last valid char.
pub fn truncate_to_tokens(text: &str, max_tokens: u32) -> String {
    let total = count_tokens(text);
    if total <= max_tokens {
        return text.to_string();
    }

    // Binary search for the right character count
    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();

    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let candidate: String = chars[..mid].iter().collect();
        if count_tokens(&candidate) <= max_tokens {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }

    chars[..lo].iter().collect()
}

// ─── Context Assembly ───

/// Format hybrid search results into a context string for the LLM prompt.
///
/// Format per SPEC.md §5.5:
/// ```text
/// [来源：title | section_path]
/// content
/// ```
pub fn assemble_context(results: &[HybridSearchResult], max_tokens: u32) -> String {
    let mut context = String::new();

    for result in results {
        let section = result
            .section_path
            .as_deref()
            .unwrap_or("（无章节信息）");

        let entry = format!(
            "[来源：{} | {}]\n{}\n\n",
            result.title, section, result.content
        );
        context.push_str(&entry);
    }

    // Truncate if exceeds budget
    truncate_to_tokens(&context, max_tokens)
}

/// Build the user prompt with context and query.
///
/// When context is empty (no search results / embedding unavailable),
/// falls back to pure conversational mode without referencing the knowledge base.
fn build_user_prompt(context: &str, query: &str) -> String {
    if context.trim().is_empty() {
        // Pure LLM chat — no knowledge base context available
        format!("用户问题：{query}\n\n请直接回答用户的问题。")
    } else {
        // RAG mode — knowledge base context available
        format!(
            "知识库检索到的相关内容：\n{context}\n\n用户问题：{query}\n\n请根据以上知识库内容回答。"
        )
    }
}

// ─── LLM Service ───

/// LLM Service — manages API config and provides RAG query capabilities.
pub struct LLMService {
    /// Current API configuration
    config: Arc<Mutex<LLMConfig>>,
    /// Path to persist config JSON (e.g. ~/.kingdee-kb/config.json)
    config_path: PathBuf,
    /// HTTP client (reusable for connection pooling)
    client: reqwest::Client,
}

impl LLMService {
    /// Create a new LLM service, loading persisted config if available.
    pub fn new(data_dir: &std::path::Path) -> Self {
        let config_path = data_dir.join("config.json");
        let config = Self::load_config(&config_path).unwrap_or_default();
        Self {
            config: Arc::new(Mutex::new(config)),
            config_path,
            client: reqwest::Client::new(),
        }
    }

    /// Load config from JSON file, returning default on any failure.
    fn load_config(path: &std::path::Path) -> Result<LLMConfig, String> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| format!("Read config failed: {}", e))?;
        serde_json::from_str(&data)
            .map_err(|e| format!("Parse config failed: {}", e))
    }

    /// Persist current config to JSON file.
    fn save_config(config: &LLMConfig, path: &std::path::Path) -> Result<(), String> {
        let data = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Serialize config failed: {}", e))?;
        std::fs::write(path, data)
            .map_err(|e| format!("Write config failed: {}", e))
    }

    /// Update the LLM configuration and persist to disk.
    pub fn set_config(&self, config: LLMConfig) -> Result<(), String> {
        let mut cfg = self.config.lock().map_err(|e| e.to_string())?;
        *cfg = config;
        Self::save_config(&*cfg, &self.config_path)?;
        Ok(())
    }

    /// Get current config (read-only clone).
    pub fn get_config(&self) -> Result<LLMConfig, String> {
        let cfg = self.config.lock().map_err(|e| e.to_string())?;
        Ok(cfg.clone())
    }

    /// Check if the LLM is configured (has API key).
    pub fn is_configured(&self) -> bool {
        self.config
            .lock()
            .map(|cfg| !cfg.api_key.is_empty())
            .unwrap_or(false)
    }

    /// Perform a RAG query: hybrid search → context assembly → LLM streaming.
    ///
    /// Returns an async stream of `StreamChunk`s. If LLM is unavailable,
    /// falls back to returning search results as a single chunk.
    ///
    /// Branches by provider: OpenAI uses /chat/completions, Anthropic uses /messages.
    pub async fn rag_query(
        &self,
        query: &str,
        project_id: Option<&str>,
        conversation_history: Vec<ChatMessage>,
        embedding: &Mutex<EmbeddingService>,
        vector_index: &Mutex<VectorIndex>,
        bm25: &Mutex<BM25Service>,
        metadata: &Mutex<MetadataStore>,
    ) -> Result<Vec<StreamChunk>, String> {
        // Step 1: Hybrid search
        let search_results = hybrid_search::hybrid_search(
            query,
            project_id,
            5, // top_k per SPEC.md
            embedding,
            vector_index,
            bm25,
            metadata,
        )?;

        // Step 2: Check if LLM is configured — fallback to search-only
        if !self.is_configured() {
            return Ok(self.fallback_response(&search_results));
        }

        // Step 3: Read config in a block scope to drop MutexGuard before .await
        let config = {
            let cfg = self.config.lock().map_err(|e| e.to_string())?;
            cfg.clone()
        };

        // Step 4: Assemble context
        let system_tokens = count_tokens(SYSTEM_PROMPT);
        let budget = config.max_tokens.saturating_sub(system_tokens + RESPONSE_TOKENS + 200);
        let context = assemble_context(&search_results, budget);
        let user_prompt = build_user_prompt(&context, query);

        // Step 5: Build messages array (common for both providers)
        let mut messages: Vec<ChatMessage> = Vec::new();
        // Include conversation history
        for msg in &conversation_history {
            messages.push(ChatMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            });
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        });

        // Step 6: Branch by provider
        match config.provider {
            LLMProvider::OpenAI => {
                self.rag_query_openai(&config, SYSTEM_PROMPT, &messages).await
            }
            LLMProvider::Anthropic => {
                self.rag_query_anthropic(&config, SYSTEM_PROMPT, &messages).await
            }
        }
    }

    /// OpenAI streaming RAG query — POST /chat/completions with OpenAI SSE format
    async fn rag_query_openai(
        &self,
        config: &LLMConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
    ) -> Result<Vec<StreamChunk>, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

        // Build messages array with system prompt as first message
        let mut api_messages = vec![serde_json::json!({
            "role": "system",
            "content": system_prompt
        })];
        for msg in messages {
            api_messages.push(serde_json::json!({
                "role": msg.role,
                "content": msg.content
            }));
        }

        let body = serde_json::json!({
            "model": config.model,
            "messages": api_messages,
            "temperature": config.temperature,
            "max_tokens": RESPONSE_TOKENS,
            "stream": true
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenAI request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(format!("OpenAI API error ({}): {}", status, body_text));
        }

        // Parse OpenAI SSE stream
        self.parse_openai_stream(response).await
    }

    /// Anthropic streaming RAG query — POST /messages with Anthropic SSE format
    async fn rag_query_anthropic(
        &self,
        config: &LLMConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
    ) -> Result<Vec<StreamChunk>, String> {
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));

        // Anthropic: system prompt is a top-level field, NOT in messages array
        // Filter out any system messages from the messages array
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| serde_json::json!({
                "role": m.role,
                "content": m.content
            }))
            .collect();

        let body = serde_json::json!({
            "model": config.model,
            "max_tokens": RESPONSE_TOKENS,
            "temperature": config.temperature,
            "system": system_prompt,
            "messages": api_messages,
            "stream": true
        });

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Anthropic request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(format!("Anthropic API error ({}): {}", status, body_text));
        }

        // Parse Anthropic SSE stream
        self.parse_anthropic_stream(response).await
    }

    /// Parse OpenAI SSE stream → Vec<StreamChunk>
    ///
    /// OpenAI format: `data: {"choices":[{"delta":{"content":"..."}}]}`
    /// End marker: `data: [DONE]` or `finish_reason: "stop"`
    async fn parse_openai_stream(
        &self,
        response: reqwest::Response,
    ) -> Result<Vec<StreamChunk>, String> {
        let mut chunks = Vec::new();
        let mut byte_stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(item) = byte_stream.next().await {
            let bytes = item.map_err(|e| format!("Stream read error: {}", e))?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();

                    if data == "[DONE]" {
                        chunks.push(StreamChunk { content: String::new(), done: true });
                        return Ok(chunks);
                    }

                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(choices) = parsed["choices"].as_array() {
                            if let Some(choice) = choices.first() {
                                if let Some(delta) = choice.get("delta") {
                                    if let Some(content) = delta["content"].as_str() {
                                        chunks.push(StreamChunk { content: content.to_string(), done: false });
                                    }
                                }
                                if choice["finish_reason"] == "stop" {
                                    chunks.push(StreamChunk { content: String::new(), done: true });
                                    return Ok(chunks);
                                }
                            }
                        }
                    }
                }
            }
        }

        if chunks.last().map(|c| c.done) != Some(true) {
            chunks.push(StreamChunk { content: String::new(), done: true });
        }
        Ok(chunks)
    }

    /// Parse Anthropic SSE stream → Vec<StreamChunk>
    ///
    /// Anthropic format: `event: content_block_delta` / `data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}`
    /// End marker: `event: message_stop`
    async fn parse_anthropic_stream(
        &self,
        response: reqwest::Response,
    ) -> Result<Vec<StreamChunk>, String> {
        let mut chunks = Vec::new();
        let mut byte_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut current_event_type = String::new();

        while let Some(item) = byte_stream.next().await {
            let bytes = item.map_err(|e| format!("Stream read error: {}", e))?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() {
                    // Empty line = end of an SSE event block
                    continue;
                }

                // Anthropic SSE uses "event:" lines to specify event type
                if let Some(event_type) = line.strip_prefix("event: ") {
                    current_event_type = event_type.trim().to_string();
                    continue;
                }

                if line.starts_with(':') {
                    continue; // comment line
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();

                    // Check for message_stop event (stream end)
                    if current_event_type == "message_stop" {
                        chunks.push(StreamChunk { content: String::new(), done: true });
                        return Ok(chunks);
                    }

                    // Parse content_block_delta for text content
                    if current_event_type == "content_block_delta" {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                            if parsed["type"] == "content_block_delta" {
                                if let Some(delta) = parsed.get("delta") {
                                    if delta["type"] == "text_delta" {
                                        if let Some(text) = delta["text"].as_str() {
                                            chunks.push(StreamChunk { content: text.to_string(), done: false });
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Also handle error events
                    if current_event_type == "error" {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                            let error_msg = parsed["error"]["message"]
                                .as_str()
                                .unwrap_or("Unknown Anthropic stream error");
                            return Err(format!("Anthropic stream error: {}", error_msg));
                        }
                    }

                    current_event_type.clear();
                }
            }
        }

        // If stream ended without message_stop
        if chunks.last().map(|c| c.done) != Some(true) {
            chunks.push(StreamChunk { content: String::new(), done: true });
        }
        Ok(chunks)
    }

    /// Non-streaming RAG query — collects all chunks into a single response.
    pub async fn rag_query_sync(
        &self,
        query: &str,
        project_id: Option<&str>,
        conversation_history: Vec<ChatMessage>,
        embedding: &Mutex<EmbeddingService>,
        vector_index: &Mutex<VectorIndex>,
        bm25: &Mutex<BM25Service>,
        metadata: &Mutex<MetadataStore>,
    ) -> Result<RAGResponse, String> {
        let search_results = hybrid_search::hybrid_search(
            query,
            project_id,
            5,
            embedding,
            vector_index,
            bm25,
            metadata,
        )?;

        if !self.is_configured() {
            let sources = search_results
                .iter()
                .map(|r| RAGSource {
                    title: r.title.clone(),
                    section_path: r.section_path.clone(),
                    content_snippet: truncate_to_tokens(&r.content, 100),
                    score: r.score,
                })
                .collect();

            return Ok(RAGResponse {
                answer: format!(
                    "知识库检索到 {} 条相关结果，但 LLM 未配置，无法生成 AI 回答。\n\n{}",
                    search_results.len(),
                    self.format_search_only_answer(&search_results)
                ),
                sources,
                llm_available: false,
            });
        }

        let chunks = self
            .rag_query(
                query,
                project_id,
                conversation_history,
                embedding,
                vector_index,
                bm25,
                metadata,
            )
            .await?;

        let answer: String = chunks.iter().map(|c| c.content.as_str()).collect();

        let sources = search_results
            .iter()
            .map(|r| RAGSource {
                title: r.title.clone(),
                section_path: r.section_path.clone(),
                content_snippet: truncate_to_tokens(&r.content, 100),
                score: r.score,
            })
            .collect();

        Ok(RAGResponse {
            answer,
            sources,
            llm_available: true,
        })
    }

    /// Simple chat completion (non-streaming, no RAG context).
    ///
    /// Sends messages directly to the LLM API and returns the response text.
    /// Used for field generation and other non-RAG tasks.
    ///
    /// Branches by provider: OpenAI uses /chat/completions, Anthropic uses /messages.
    pub async fn chat_completion(
        &self,
        messages: &[ChatMessage],
        config: &LLMConfig,
    ) -> Result<String, String> {
        if config.api_key.is_empty() {
            return Err("LLM API key not configured".to_string());
        }

        match config.provider {
            LLMProvider::OpenAI => self.chat_completion_openai(messages, config).await,
            LLMProvider::Anthropic => self.chat_completion_anthropic(messages, config).await,
        }
    }

    /// OpenAI non-streaming chat completion — POST /chat/completions
    async fn chat_completion_openai(
        &self,
        messages: &[ChatMessage],
        config: &LLMConfig,
    ) -> Result<String, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content
                })
            })
            .collect();

        let body = serde_json::json!({
            "model": config.model,
            "messages": api_messages,
            "temperature": config.temperature,
            "stream": false
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenAI request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(format!("OpenAI API error ({}): {}", status, body_text));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse OpenAI response: {}", e))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            return Err("OpenAI returned empty response".to_string());
        }

        Ok(content)
    }

    /// Anthropic non-streaming chat completion — POST /messages
    ///
    /// Anthropic requires `system` as a top-level field, not in messages.
    /// Response format: `{"content":[{"type":"text","text":"..."}]}`
    async fn chat_completion_anthropic(
        &self,
        messages: &[ChatMessage],
        config: &LLMConfig,
    ) -> Result<String, String> {
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));

        // Extract system prompt from messages (if any) and filter it out
        let system_prompt: String = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect::<Vec<&str>>()
            .join("\n");

        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| serde_json::json!({
                "role": m.role,
                "content": m.content
            }))
            .collect();

        let mut body = serde_json::json!({
            "model": config.model,
            "max_tokens": RESPONSE_TOKENS,
            "temperature": config.temperature,
            "messages": api_messages
        });

        // Anthropic: system is a top-level field, required even if empty
        if !system_prompt.is_empty() {
            body["system"] = serde_json::json!(system_prompt);
        }

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Anthropic request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(format!("Anthropic API error ({}): {}", status, body_text));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Anthropic response: {}", e))?;

        // Anthropic response: content[0].text
        let content = json["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            return Err("Anthropic returned empty response".to_string());
        }

        Ok(content)
    }

    /// Test LLM API connectivity without requiring embedding or RAG pipeline.
    ///
    /// Sends a minimal chat completion request (max_tokens: 5) to verify
    /// that the API key and endpoint are valid. Returns Ok with a success
    /// message or Err with a descriptive error.
    pub async fn test_connection(&self) -> Result<String, String> {
        let config = {
            let cfg = self.config.lock().map_err(|e| e.to_string())?;
            if cfg.api_key.is_empty() {
                return Err("API Key 未配置".to_string());
            }
            cfg.clone()
        };

        match config.provider {
            LLMProvider::OpenAI => {
                let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": config.model,
                    "messages": [{"role": "user", "content": "Hi"}],
                    "max_tokens": 5,
                    "temperature": 0.0
                });

                let response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", config.api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| format!("连接失败：{}", e))?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "<unreadable>".to_string());
                    return Err(format!("API 返回错误 ({})：{}", status, body_text));
                }

                Ok(format!("连接成功（openai / {}）", config.model))
            }
            LLMProvider::Anthropic => {
                let url = format!("{}/messages", config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": config.model,
                    "max_tokens": 5,
                    "temperature": 0.0,
                    "messages": [{"role": "user", "content": "Hi"}]
                });

                let response = self
                    .client
                    .post(&url)
                    .header("x-api-key", &config.api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
                    .header("anthropic-dangerous-direct-browser-access", "true")
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| format!("连接失败：{}", e))?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "<unreadable>".to_string());
                    return Err(format!("API 返回错误 ({})：{}", status, body_text));
                }

                Ok(format!("连接成功（anthropic / {}）", config.model))
            }
        }
    }

    /// Generate a fallback response when LLM is unavailable.
    fn fallback_response(&self, results: &[HybridSearchResult]) -> Vec<StreamChunk> {
        let answer = format!(
            "⚠️ LLM 未配置（请在设置中填写 API Key），以下为知识库检索结果：\n\n{}",
            self.format_search_only_answer(results)
        );

        vec![StreamChunk {
            content: answer,
            done: true,
        }]
    }

    /// Format search results as a readable text-only answer.
    fn format_search_only_answer(&self, results: &[HybridSearchResult]) -> String {
        if results.is_empty() {
            return "知识库中暂无相关内容。".to_string();
        }

        let mut output = String::new();
        for (i, r) in results.iter().enumerate() {
            let section = r
                .section_path
                .as_deref()
                .unwrap_or("（无章节信息）");
            output.push_str(&format!(
                "**{}. {}** （来源：{} | {}）\n{}\n\n",
                i + 1,
                r.title,
                r.title,
                section,
                truncate_to_tokens(&r.content, 200)
            ));
        }
        output
    }
}
