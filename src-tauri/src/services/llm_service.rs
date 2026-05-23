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
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

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

/// Token threshold for conversation compression
const COMPRESS_THRESHOLD: u32 = 2000;

/// Number of recent message pairs to keep uncompressed during compression
const KEEP_LAST_PAIRS: usize = 2;

/// Half-life for temporal decay of memory scores (in days).
/// After 30 days, a memory's relevance score is halved.
const MEMORY_HALF_LIFE_DAYS: f64 = 30.0;

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
    /// Local model (Ollama, llama.cpp, etc.) — no API key needed, uses local server
    #[serde(rename = "local")]
    Local,
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
    /// Thinking/reasoning text (e.g., DeepSeek R1's reasoning_content).
    /// Emitted only when the model produces it; most chunks have None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
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

/// Apply temporal decay to memory search results.
///
/// Inspired by OpenClaw's temporal-decay.ts — older memories get exponentially
/// lower effective scores, so top_k naturally filters out stale context.
/// Half-life = 30 days: after 30 days score is halved, after 60 days quartered.
fn apply_memory_temporal_decay(
    results: &mut Vec<HybridSearchResult>,
    metadata: &Mutex<MetadataStore>,
) {
    let half_life_days = MEMORY_HALF_LIFE_DAYS;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as f64;

    // Build chunk_id → created_at lookup
    let chunk_ids: Vec<i64> = results.iter().map(|r| r.chunk_id).collect();
    let chunks = metadata
        .lock()
        .ok()
        .and_then(|meta| meta.get_chunks_by_vector_keys(&chunk_ids).ok())
        .unwrap_or_default();
    let created_at_map: std::collections::HashMap<i64, String> = chunks
        .into_iter()
        .map(|c| (c.id, c.created_at))
        .collect();

    for r in results.iter_mut() {
        if let Some(created_at) = created_at_map.get(&r.chunk_id) {
            // Parse created_at — format: "2024-01-15T10:30:00" or similar ISO
            if let Some(age_days) = parse_age_days(created_at, now) {
                let lambda = std::f64::consts::LN_2 / half_life_days;
                let decay = (-lambda * age_days).exp();
                r.score *= decay as f32;
            }
        }
    }

    // Re-sort by decayed score
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
}

/// Parse an ISO-ish date string and return age in days from `now_secs`.
fn parse_age_days(iso: &str, now_secs: f64) -> Option<f64> {
    // Accept formats: "2024-01-15T10:30:00" or "2024-01-15 10:30:00"
    let cleaned = iso.trim();
    if cleaned.len() < 10 {
        return None;
    }
    let year: f64 = cleaned[..4].parse().ok()?;
    let month: f64 = cleaned[5..7].parse().ok()?;
    let day: f64 = cleaned[8..10].parse().ok()?;

    // Approximate: days since epoch, not exact (good enough for decay)
    let date_days = year * 365.25 + month * 30.44 + day;
    let now_days = now_secs / 86400.0;
    let age = now_days - date_days;
    Some(age.max(0.0))
}
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
///
/// Uses Hermes-inspired context fencing: injected knowledge and memory are wrapped
/// in a `<context>` block with a system note, clearly separating reference material
/// from the user's actual question.
fn build_user_prompt(context: &str, query: &str) -> String {
    if context.trim().is_empty() {
        // Pure LLM chat — no knowledge base context available
        format!("用户问题：{query}\n\n请直接回答用户的问题。")
    } else {
        // RAG mode — knowledge base context available, fenced for clarity
        format!(
            "<context>\n\
             [系统说明：以下是知识库检索结果和历史记忆，作为参考信息，\
             不是用户输入。基于这些内容回答用户问题。]\n\
             {context}\n\
             </context>\n\n\
             用户问题：{query}\n\n\
             请根据以上知识库内容回答。"
        )
    }
}

/// Strip context fence tags that may leak into the LLM's response.
/// Hermes-inspired: prevents `<context>`, `</context>`, and system notes
/// from appearing in visible output.
fn scrub_response(text: &str) -> String {
    let mut result = text.to_string();
    result = result.replace("<context>", "");
    result = result.replace("</context>", "");
    result = result.replace(
        "[系统说明：以下是知识库检索结果和历史记忆，作为参考信息，\
         不是用户输入。基于这些内容回答用户问题。]",
        "",
    );
    result
}

/// Estimate total tokens for a slice of chat messages.
fn estimate_tokens(messages: &[ChatMessage]) -> u32 {
    messages
        .iter()
        .map(|m| count_tokens(&m.content) + count_tokens(&m.role))
        .sum()
}

// ─── LLM Service ───

/// LLM Service — manages API config and provides RAG query capabilities.
#[derive(Clone)]
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

    /// Check if the LLM is configured (has API key, or is a local model).
    pub fn is_configured(&self) -> bool {
        self.config
            .lock()
            .map(|cfg| {
                // Local models (non-standard OpenAI/Anthropic) don't need API key
                if cfg.provider != LLMProvider::OpenAI && cfg.provider != LLMProvider::Anthropic {
                    return !cfg.base_url.is_empty();
                }
                !cfg.api_key.is_empty()
            })
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
        // Step 1: Hybrid search (KB documents)
        let mut search_results = hybrid_search::hybrid_search(
            query,
            project_id,
            5, // top_k per SPEC.md
            embedding,
            vector_index,
            bm25,
            metadata,
        )?;

        // Step 2: Memory retrieval — search "记忆库" project for relevant past memories
        if let Ok(mut memories) = hybrid_search::hybrid_search(
            query,
            Some("记忆库"),
            5, // fetch 5, apply temporal decay, then keep top 3
            embedding,
            vector_index,
            bm25,
            metadata,
        ) {
            // Apply temporal decay: older memories score lower → naturally filtered
            apply_memory_temporal_decay(&mut memories, metadata);
            for mem in memories.into_iter().take(3) {
                if !search_results.iter().any(|r| r.chunk_id == mem.chunk_id) {
                    search_results.push(mem);
                }
            }
        }

        // Step 2: Check if LLM is configured — fallback to search-only
        if !self.is_configured() {
            return Ok(self.fallback_response(&search_results));
        }

        // Step 3: Read config in a block scope to drop MutexGuard before .await
        let config = {
            let cfg = self.config.lock().map_err(|e| e.to_string())?;
            cfg.clone()
        };

        // Step 4: Compress conversation history if it exceeds token threshold
        // (OpenCode-inspired: summarize older turns, keep last 2 pairs verbatim)
        let compressed = self.compress_conversation(&conversation_history).await;
        let compressed_history = match compressed {
            Ok(c) => c,
            Err(_) => conversation_history,
        };

        // Step 5: Assemble context
        let system_tokens = count_tokens(SYSTEM_PROMPT);
        let budget = config.max_tokens.saturating_sub(system_tokens + RESPONSE_TOKENS + 200);
        let context = assemble_context(&search_results, budget);
        let user_prompt = build_user_prompt(&context, query);

        // Step 6: Build messages array (common for both providers)
        let mut messages: Vec<ChatMessage> = Vec::new();
        // Include compressed conversation history
        for msg in &compressed_history {
            messages.push(ChatMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            });
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        });

        // Step 7: Branch by provider (Local uses OpenAI-compatible protocol)
        match config.provider {
            LLMProvider::OpenAI | LLMProvider::Local => {
                self.rag_query_openai(&config, SYSTEM_PROMPT, &messages).await
            }
            LLMProvider::Anthropic => {
                self.rag_query_anthropic(&config, SYSTEM_PROMPT, &messages).await
            }
        }
    }

    /// OpenAI streaming RAG query — POST /chat/completions with OpenAI SSE format
    ///
    /// Uses `reqwest_eventsource::EventSource` for robust SSE parsing.
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

        let request = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create EventSource: {}", e))?;

        let mut chunks = Vec::new();

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        chunks.push(StreamChunk { content: String::new(), done: true, thinking: None });
                        return Ok(chunks);
                    }

                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                        // Extract reasoning/thinking content (e.g. DeepSeek R1)
                        if let Some(reasoning) =
                            parsed["choices"][0]["delta"]["reasoning_content"].as_str()
                        {
                            if !reasoning.is_empty() {
                                chunks.push(StreamChunk {
                                    content: String::new(),
                                    done: false,
                                    thinking: Some(reasoning.to_string()),
                                });
                            }
                        }
                        // Extract text content from delta
                        if let Some(content) =
                            parsed["choices"][0]["delta"]["content"].as_str()
                        {
                            let cleaned = scrub_response(content);
                            if !cleaned.is_empty() {
                                chunks.push(StreamChunk { content: cleaned, done: false, thinking: None });
                            }
                        }
                        // Check finish_reason
                        if let Some(reason) =
                            parsed["choices"][0]["finish_reason"].as_str()
                        {
                            if !reason.is_empty() && reason != "null" && reason != "null" {
                                chunks.push(StreamChunk { content: String::new(), done: true, thinking: None });
                                return Ok(chunks);
                            }
                        }
                    }
                }
                Ok(Event::Open) => {
                    // Connection established — no action needed
                }
                Err(e) => {
                    // If we already have some content, return it gracefully
                    if !chunks.is_empty() {
                        chunks.push(StreamChunk { content: String::new(), done: true, thinking: None });
                        return Ok(chunks);
                    }
                    return Err(format!("SSE error: {}", e));
                }
            }
        }

        if chunks.last().map(|c| c.done) != Some(true) {
            chunks.push(StreamChunk { content: String::new(), done: true, thinking: None });
        }
        Ok(chunks)
    }

    /// Anthropic streaming RAG query — POST /messages with Anthropic SSE format
    ///
    /// Uses `reqwest_eventsource::EventSource` for robust SSE parsing.
    /// Anthropic sends events: `content_block_delta`, `message_delta`, `message_stop`.
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

        let request = self
            .client
            .post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("Content-Type", "application/json")
            .json(&body);

        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create EventSource: {}", e))?;

        let mut chunks = Vec::new();

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    match msg.event.as_str() {
                        "content_block_delta" => {
                            if let Ok(data) =
                                serde_json::from_str::<serde_json::Value>(&msg.data)
                            {
                                if data["type"] == "content_block_delta" {
                                    if let Some(delta) = data.get("delta") {
                                        if delta["type"] == "text_delta" {
                                            if let Some(text) = delta["text"].as_str() {
                                                let cleaned = scrub_response(text);
                                                if !cleaned.is_empty() {
                                                    chunks.push(StreamChunk { content: cleaned, done: false, thinking: None });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "message_delta" => {
                            chunks.push(StreamChunk { content: String::new(), done: true, thinking: None });
                            return Ok(chunks);
                        }
                        "message_stop" => {
                            // Safety net: some Anthropic-compatible endpoints
                            // skip message_delta — ensure done is emitted
                            chunks.push(StreamChunk { content: String::new(), done: true, thinking: None });
                            return Ok(chunks);
                        }
                        "error" => {
                            if let Ok(data) =
                                serde_json::from_str::<serde_json::Value>(&msg.data)
                            {
                                let err_msg = data["error"]["message"]
                                    .as_str()
                                    .unwrap_or("Unknown Anthropic stream error");
                                return Err(format!("Anthropic stream error: {}", err_msg));
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Open) => {
                    // Connection established — no action needed
                }
                Err(e) => {
                    // If we already have content, return it gracefully
                    if !chunks.is_empty() {
                        chunks.push(StreamChunk { content: String::new(), done: true, thinking: None });
                        return Ok(chunks);
                    }
                    return Err(format!("SSE error: {}", e));
                }
            }
        }

        if chunks.last().map(|c| c.done) != Some(true) {
            chunks.push(StreamChunk { content: String::new(), done: true, thinking: None });
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
            LLMProvider::OpenAI | LLMProvider::Local => self.chat_completion_openai(messages, config).await,
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

    /// RAG query with channel-based streaming.
    ///
    /// Same as `rag_query()` but sends each `StreamChunk` through the channel
    /// as it arrives from the LLM, enabling real-time frontend streaming.
    /// The caller is responsible for reading all chunks from the receiver.
    ///
    /// If `precomputed_results` is provided, skips the hybrid search step
    /// (useful when the caller already ran search for source extraction).
    pub async fn rag_query_to_sender(
        &self,
        query: &str,
        project_id: Option<&str>,
        conversation_history: Vec<ChatMessage>,
        embedding: &Mutex<EmbeddingService>,
        vector_index: &Mutex<VectorIndex>,
        bm25: &Mutex<BM25Service>,
        metadata: &Mutex<MetadataStore>,
        tx: mpsc::Sender<StreamChunk>,
        precomputed_results: Option<Vec<HybridSearchResult>>,
    ) -> Result<(), String> {
        // Step 1: Hybrid search (skip if precomputed)
        let mut search_results: Vec<HybridSearchResult> = match precomputed_results {
            Some(results) => results,
            None => hybrid_search::hybrid_search(
                query,
                project_id,
                5,
                embedding,
                vector_index,
                bm25,
                metadata,
            )?,
        };

        // Step 1b: Memory retrieval — search "记忆库" project for relevant past memories
        if let Ok(mut memories) = hybrid_search::hybrid_search(
            query,
            Some("记忆库"),
            5, // fetch 5, apply temporal decay, then keep top 3
            embedding,
            vector_index,
            bm25,
            metadata,
        ) {
            apply_memory_temporal_decay(&mut memories, metadata);
            for mem in memories.into_iter().take(3) {
                if !search_results.iter().any(|r| r.chunk_id == mem.chunk_id) {
                    search_results.push(mem);
                }
            }
        }

        // Step 2: Check if LLM is configured
        if !self.is_configured() {
            let answer = self.fallback_response(&search_results);
            for chunk in answer {
                let _ = tx.send(chunk).await;
            }
            return Ok(());
        }

        // Step 3: Read config
        let config = {
            let cfg = self.config.lock().map_err(|e| e.to_string())?;
            cfg.clone()
        };

        // Compress conversation if too long (OpenCode-inspired)
        let compressed = self.compress_conversation(&conversation_history).await;
        let conversation_history = match compressed {
            Ok(c) => c,
            Err(_) => conversation_history,
        };

        // Step 4: Assemble context
        let system_tokens = count_tokens(SYSTEM_PROMPT);
        let budget = config.max_tokens.saturating_sub(system_tokens + RESPONSE_TOKENS + 200);
        let context = assemble_context(&search_results, budget);
        let user_prompt = build_user_prompt(&context, query);

        // Step 5: Build messages array
        let mut messages: Vec<ChatMessage> = Vec::new();
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

        // Step 6: Branch by provider and stream to channel
        // Branch by provider (Local uses OpenAI-compatible protocol)
        match config.provider {
            LLMProvider::OpenAI | LLMProvider::Local => {
                self.stream_openai_to_sender(&config, SYSTEM_PROMPT, &messages, &tx)
                    .await
            }
            LLMProvider::Anthropic => {
                self.stream_anthropic_to_sender(&config, SYSTEM_PROMPT, &messages, &tx)
                    .await
            }
        }
    }

    /// Stream OpenAI response to channel sender (real-time).
    async fn stream_openai_to_sender(
        &self,
        config: &LLMConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
        tx: &mpsc::Sender<StreamChunk>,
    ) -> Result<(), String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

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

        let request = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create EventSource: {}", e))?;

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        let _ = tx.send(StreamChunk { content: String::new(), done: true, thinking: None }).await;
                        return Ok(());
                    }

                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                        // Extract reasoning/thinking content (e.g. DeepSeek R1)
                        if let Some(reasoning) = parsed["choices"][0]["delta"]["reasoning_content"].as_str() {
                            if !reasoning.is_empty() {
                                let _ = tx.send(StreamChunk {
                                    content: String::new(),
                                    done: false,
                                    thinking: Some(reasoning.to_string()),
                                }).await;
                            }
                        }
                        // Extract visible text content
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            let cleaned = scrub_response(content);
                            if !cleaned.is_empty() {
                                let _ = tx.send(StreamChunk { content: cleaned, done: false, thinking: None }).await;
                            }
                        }
                        if let Some(reason) = parsed["choices"][0]["finish_reason"].as_str() {
                            if !reason.is_empty() && reason != "null" {
                                let _ = tx.send(StreamChunk { content: String::new(), done: true, thinking: None }).await;
                                return Ok(());
                            }
                        }
                    }
                }
                Ok(Event::Open) => {}
                Err(e) => {
                    return Err(format!("SSE error: {}", e));
                }
            }
        }

        // Stream ended without explicit done marker
        let _ = tx.send(StreamChunk { content: String::new(), done: true, thinking: None }).await;
        Ok(())
    }

    /// Stream Anthropic response to channel sender (real-time).
    async fn stream_anthropic_to_sender(
        &self,
        config: &LLMConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
        tx: &mpsc::Sender<StreamChunk>,
    ) -> Result<(), String> {
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));

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

        let request = self
            .client
            .post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("Content-Type", "application/json")
            .json(&body);

        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create EventSource: {}", e))?;

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    match msg.event.as_str() {
                        "content_block_delta" => {
                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                                if data["type"] == "content_block_delta" {
                                    if let Some(delta) = data.get("delta") {
                                        if delta["type"] == "text_delta" {
                                            if let Some(text) = delta["text"].as_str() {
                                                let cleaned = scrub_response(text);
                                                if !cleaned.is_empty() {
                                                    let _ = tx.send(StreamChunk { content: cleaned, done: false, thinking: None }).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "message_delta" => {
                            let _ = tx.send(StreamChunk { content: String::new(), done: true, thinking: None }).await;
                            return Ok(());
                        }
                        "message_stop" => {
                            let _ = tx.send(StreamChunk { content: String::new(), done: true, thinking: None }).await;
                            return Ok(());
                        }
                        "error" => {
                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                                let err_msg = data["error"]["message"]
                                    .as_str()
                                    .unwrap_or("Unknown Anthropic stream error");
                                return Err(format!("Anthropic stream error: {}", err_msg));
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Open) => {}
                Err(e) => {
                    return Err(format!("SSE error: {}", e));
                }
            }
        }

        // Stream ended without done marker
        let _ = tx.send(StreamChunk { content: String::new(), done: true, thinking: None }).await;
        Ok(())
    }

    /// Compress conversation history when it exceeds token threshold.
    ///
    /// Strategy (inspired by OpenCode's compaction):
    /// 1. Estimate total tokens; if below `COMPRESS_THRESHOLD`, return as-is
    /// 2. Split into **head** (older turns) and **tail** (keep last `KEEP_LAST_PAIRS` pairs)
    /// 3. Summarize the head via LLM into a structured summary
    /// 4. Return: [summary_system_msg, ...tail] — summary injected as system message
    async fn compress_conversation(
        &self,
        conversation: &[ChatMessage],
    ) -> Result<Vec<ChatMessage>, String> {
        let total_tokens: u32 = conversation
            .iter()
            .map(|m| count_tokens(&m.content))
            .sum();

        if total_tokens <= COMPRESS_THRESHOLD || !self.is_configured() {
            return Ok(conversation.to_vec());
        }

        // Walk backwards from the end to find split point:
        // keep last KEEP_LAST_PAIRS user+assistant pairs (+ any trailing non-user msgs)
        let mut pairs_found = 0usize;
        let split_idx = {
            let mut idx = conversation.len();
            for (i, msg) in conversation.iter().enumerate().rev() {
                if msg.role == "user" {
                    pairs_found += 1;
                    if pairs_found > KEEP_LAST_PAIRS {
                        idx = i;
                        break;
                    }
                }
            }
            idx
        };

        let (head, tail) = conversation.split_at(split_idx);

        if head.is_empty() {
            return Ok(conversation.to_vec());
        }

        // Build head text for summarization
        let head_text: String = head
            .iter()
            .map(|m| format!("**{}**: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let config = self.get_config()?;

        let summary_prompt = format!(
            "从以下对话中提取关键信息，保持简洁的要点格式：\n\
             \n\
             ## 项目背景\n\
             ## 关键决策\n\
             ## 待办事项\n\
             \n\
             如果没有相关信息就留空该章节。\n\
             \n---\n\n{}",
            head_text
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是一个对话摘要助手。提取关键信息，保持项目名、决策、技术选型、业务规则等。\
                         直接输出结构化摘要，不要前缀。"
                    .to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: summary_prompt,
            },
        ];

        match self.chat_completion(&messages, &config).await {
            Ok(summary) => {
                let mut result = Vec::new();
                result.push(ChatMessage {
                    role: "system".to_string(),
                    content: format!("【历史对话摘要】\n{}", summary.trim()),
                });
                result.extend(tail.iter().cloned());
                Ok(result)
            }
            Err(e) => {
                eprintln!(
                    "[Compress] LLM summarization failed: {} — keeping full history",
                    e
                );
                Ok(conversation.to_vec())
            }
        }
    }

    /// Test LLM API connectivity without requiring embedding or RAG pipeline.
    ///
    /// Sends a minimal chat completion request (max_tokens: 5) to verify
    /// that the API key and endpoint are valid. Returns Ok with a success
    /// message or Err with a descriptive error.
    pub async fn test_connection(&self) -> Result<String, String> {
        let config = {
            let cfg = self.config.lock().map_err(|e| e.to_string())?;
            // Local models (non-standard) are allowed to have empty API key
            let is_local = cfg.provider != LLMProvider::OpenAI && cfg.provider != LLMProvider::Anthropic;
            if cfg.api_key.is_empty() && !is_local {
                return Err("API Key 未配置".to_string());
            }
            cfg.clone()
        };

        match config.provider {
            LLMProvider::OpenAI | LLMProvider::Local => {
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
    pub(crate) fn fallback_response(&self, results: &[HybridSearchResult]) -> Vec<StreamChunk> {
        let answer = format!(
            "⚠️ LLM 未配置（请在设置中填写 API Key），以下为知识库检索结果：\n\n{}",
            self.format_search_only_answer(results)
        );

        vec![StreamChunk {
            content: answer,
            done: true,
            thinking: None,
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
