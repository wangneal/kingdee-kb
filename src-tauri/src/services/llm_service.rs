//! LLM Service — OpenAI-compatible Chat Completions client with SSE streaming
//!
//! Provides the full RAG pipeline:
//!   embed query → hybrid search → context assembly → LLM completion (SSE)
//!
//! Graceful fallback: when LLM is unavailable, returns search results only.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
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
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Default model
const DEFAULT_MODEL: &str = "gpt-4o";

// ─── Configuration ───

/// LLM provider configuration (OpenAI-compatible API)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    /// API key for authentication
    pub api_key: String,
    /// Base URL (default: https://api.openai.com/v1)
    pub base_url: String,
    /// Model name (default: gpt-4o)
    pub model: String,
    /// Max context window in tokens (default: 4096)
    pub max_tokens: u32,
    /// Temperature for generation (default: 0.3)
    pub temperature: f32,
}

impl Default for LLMConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
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
fn build_user_prompt(context: &str, query: &str) -> String {
    format!(
        "知识库检索到的相关内容：\n{context}\n\n用户问题：{query}\n\n请根据以上知识库内容回答。"
    )
}

// ─── LLM Service ───

/// LLM Service — manages API config and provides RAG query capabilities.
pub struct LLMService {
    /// Current API configuration
    config: Arc<Mutex<LLMConfig>>,
    /// HTTP client (reusable for connection pooling)
    client: reqwest::Client,
}

impl LLMService {
    /// Create a new LLM service with default config.
    pub fn new() -> Self {
        Self {
            config: Arc::new(Mutex::new(LLMConfig::default())),
            client: reqwest::Client::new(),
        }
    }

    /// Update the LLM configuration.
    pub fn set_config(&self, config: LLMConfig) -> Result<(), String> {
        let mut cfg = self.config.lock().map_err(|e| e.to_string())?;
        *cfg = config;
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

        // Step 3: Assemble context (reserve tokens for system prompt + response)
        // Use block scope to ensure MutexGuard is dropped before any .await
        let (api_key, base_url, model, temperature, max_ctx) = {
            let config = self.config.lock().map_err(|e| e.to_string())?;
            (
                config.api_key.clone(),
                config.base_url.clone(),
                config.model.clone(),
                config.temperature,
                config.max_tokens,
            )
        }; // config MutexGuard dropped here

        // Token budget: total - system_prompt - response_reserve - user_prompt_overhead
        let system_tokens = count_tokens(SYSTEM_PROMPT);
        let budget = max_ctx.saturating_sub(system_tokens + RESPONSE_TOKENS + 200);
        let context = assemble_context(&search_results, budget);

        // Step 4: Build messages
        let user_prompt = build_user_prompt(&context, query);
        let mut messages = vec![serde_json::json!({
            "role": "system",
            "content": SYSTEM_PROMPT
        })];

        // Include conversation history
        for msg in &conversation_history {
            messages.push(serde_json::json!({
                "role": msg.role,
                "content": msg.content
            }));
        }

        messages.push(serde_json::json!({
            "role": "user",
            "content": user_prompt
        }));

        // Step 5: Call LLM API with streaming
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        let body = serde_json::json!({
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": RESPONSE_TOKENS,
            "stream": true
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("LLM request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(format!(
                "LLM API error ({}): {}",
                status, body_text
            ));
        }

        // Step 6: Parse SSE stream
        let mut chunks = Vec::new();
        let mut byte_stream = response.bytes_stream();

        let mut buffer = String::new();

        while let Some(item) = byte_stream.next().await {
            let bytes = item.map_err(|e| format!("Stream read error: {}", e))?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            // Process complete SSE lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();

                    if data == "[DONE]" {
                        chunks.push(StreamChunk {
                            content: String::new(),
                            done: true,
                        });
                        return Ok(chunks);
                    }

                    // Parse the JSON chunk
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(choices) = parsed["choices"].as_array() {
                            if let Some(choice) = choices.first() {
                                if let Some(delta) = choice.get("delta") {
                                    if let Some(content) = delta["content"].as_str() {
                                        chunks.push(StreamChunk {
                                            content: content.to_string(),
                                            done: false,
                                        });
                                    }
                                }

                                // Check for finish_reason
                                if choice["finish_reason"] == "stop" {
                                    chunks.push(StreamChunk {
                                        content: String::new(),
                                        done: true,
                                    });
                                    return Ok(chunks);
                                }
                            }
                        }
                    }
                }
            }
        }

        // If we get here without [DONE], add a final done chunk
        if chunks.last().map(|c| c.done) != Some(true) {
            chunks.push(StreamChunk {
                content: String::new(),
                done: true,
            });
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
    pub async fn chat_completion(
        &self,
        messages: &[ChatMessage],
        config: &LLMConfig,
    ) -> Result<String, String> {
        if config.api_key.is_empty() {
            return Err("LLM API key not configured".to_string());
        }

        let url = format!(
            "{}/chat/completions",
            config.base_url.trim_end_matches('/')
        );

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
            .map_err(|e| format!("LLM request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(format!("LLM API error ({}): {}", status, body_text));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            return Err("LLM returned empty response".to_string());
        }

        Ok(content)
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

impl Default for LLMService {
    fn default() -> Self {
        Self::new()
    }
}
