//! LLM 服务 — 支持 SSE 流式的多协议 LLM 客户端
//!
//! 支持 OpenAI（Chat Completions）和 Anthropic（Messages）协议。
//! 用户在设置中选择提供商；后端直接使用该提供商的原生协议 — 无需协议转换。
//!
//! 提供完整的 RAG 管道：
//!   嵌入查询 → 混合搜索 → 上下文组装 → LLM 补全（SSE）
//!
//! 优雅回退：当 LLM 不可用时，仅返回搜索结果。

use futures_util::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::services::agent_timeout::{LLM_CALL_TIMEOUT_SECS, LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS, MAX_RETRIES, RETRY_BASE_DELAY_MS};

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::hybrid_search::{self, HybridSearchResult};
use crate::services::llm_providers::{LLMProviderConfig, LLMProtocol, LLMProviderManager};
use crate::services::metadata::MetadataStore;
use crate::services::vector_index::VectorIndex;

// ─── 常量 ───

/// 系统提示词 — ERP 顾问知识助手，带有反幻觉防护
///
/// 核心原则：
/// 1. 严谨的质量审计员 — 默认拒绝不合理二开
/// 2. 优先标准功能 — Best Practices > Custom Dev
/// 3. 不允许编造 — 找不到就说找不到
/// 4. 来源标注 — 每段回答必须有出处
const SYSTEM_PROMPT: &str = "\
你是一个金蝶ERP实施顾问知识助手。你的核心职责是提供严谨、可落地的实施建议。\n\
\n\
【反二开蔓延规则 — 严格遵守】\n\
1. 你的角色是严谨的质量审计员，不是推销员。\n\
2. 当用户提出需求时，**默认立场是拒绝不合理的二次开发**。\n\
3. 在推荐任何二开方案前，必须先检查标准功能(Best Practices)是否有替代方案。\n\
4. 如果标准功能确实无法满足，明确标记为 [Gap]，并说明是配置项差异还是需评估范围变更。\n\
5. 禁止编造不存在的系统功能、BAPI、配置路径或单据类型。\n\
6. 不得为了讨好用户而顺着不切实际的需求编造方案。\n\
\n\
【回答质量要求】\n\
1. 基于知识库中的本地文档回答，标注具体来源。\n\
2. 当知识库中无相关信息时，明确说明「知识库中暂无相关内容」。\n\
3. 回答结构：先说结论 → 再给依据 → 最后给出操作建议。\n\
4. 涉及配置时，写全路径（如：系统管理→基础资料→科目→新建）。\n\
5. 禁止使用「实现高效管理」「优化业务流程」等无具体操作的空话。\n\
\n\
【来源标注格式】\n\
回答末尾标注：(来源：[文档名称].md)";

/// 文档生成的系统提示词 — 反模糊结构约束
///
/// 遵循四段结构：As-Is → To-Be → Gap → Document
const DOC_GEN_SYSTEM_PROMPT: &str = "\
你是一个金蝶ERP实施文档撰写助手。\n\
\n\
【输出结构强制约束】\n\
所有生成的内容必须严格按以下四段结构输出：\n\
1.【现有线下流程 As-Is】— 描述客户当前的业务操作模式\n\
2.【系统标准流程 To-Be】— 描述金蝶系统中的标准解决方案\n\
3.【差异配置点】— 按「配置路径: 配置值」格式列出具体的系统配置项\n\
4.【对应系统单据类型】— 涉及的单据名称及单据编号规则\n\
\n\
【禁止事项】\n\
- 禁止「实现高效管理」「优化采购流程」等无具体操作步骤的套话\n\
- 禁止使用模糊动词如「加强」「提升」「优化」而不说明具体怎么做\n\
- 每段必须有具体的系统操作路径、配置参数或单据示例\n\
- 如果是 Gap，必须说明是标准不支持还是需要额外配置";

/// 默认上下文窗口大小（token 数）
const DEFAULT_MAX_CONTEXT_TOKENS: u32 = 4096;

/// 为助手响应保留的 token 数
const RESPONSE_TOKENS: u32 = 1024;

/// 对话压缩的 token 阈值
const COMPRESS_THRESHOLD: u32 = 2000;

/// 压缩期间保持未压缩的最近消息对数
const KEEP_LAST_PAIRS: usize = 2;

/// 记忆分数时间衰减的半衰期（天）。
/// 30 天后，记忆的相关性分数减半。
const MEMORY_HALF_LIFE_DAYS: f64 = 30.0;

/// 默认 OpenAI 基础 URL
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// 默认 OpenAI 模型
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o";

/// 默认 Anthropic 基础 URL
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";

/// 默认 Anthropic 模型
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-3-5-sonnet-20241022";

/// Anthropic API 版本头
const ANTHROPIC_VERSION: &str = "2023-06-01";

// ─── 重试工具函数 ───

/// 带指数退避的异步重试包装器
///
/// 对于瞬时错误（网络超时、429 速率限制、5xx 服务器错误）自动重试。
/// 对于永久性错误（401 认证失败、400 请求格式错误）立即返回错误。
async fn with_retry<F, Fut, T, E>(operation_name: &str, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut last_error = None;

    for attempt in 0..=MAX_RETRIES {
        match f().await {
            Ok(result) => {
                if attempt > 0 {
                    info!("{}: 成功（第{}次重试）", operation_name, attempt);
                }
                return Ok(result);
            }
            Err(e) => {
                let error_msg = e.to_string();

                // 检查是否为永久性错误（不应重试）
                if is_permanent_error(&error_msg) {
                    warn!("{}: 永久性错误，不重试: {}", operation_name, error_msg);
                    return Err(e);
                }

                last_error = Some(e);

                if attempt < MAX_RETRIES {
                    let delay = Duration::from_millis(RETRY_BASE_DELAY_MS * 2u64.pow(attempt));
                    warn!(
                        "{}: 第{}次尝试失败，{}ms 后重试: {}",
                        operation_name,
                        attempt + 1,
                        delay.as_millis(),
                        error_msg
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    warn!("{}: 所有{}次重试均失败", operation_name, MAX_RETRIES + 1);
                }
            }
        }
    }

    Err(last_error.unwrap())
}

/// 带指数退避的同步重试包装器
///
/// 用于 `generate_text_sync` 等阻塞调用。
fn with_retry_sync<F, T, E>(operation_name: &str, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
    E: std::fmt::Display,
{
    let mut last_error = None;

    for attempt in 0..=MAX_RETRIES {
        match f() {
            Ok(result) => {
                if attempt > 0 {
                    info!("{}: 成功（第{}次重试）", operation_name, attempt);
                }
                return Ok(result);
            }
            Err(e) => {
                let error_msg = e.to_string();

                // 检查是否为永久性错误（不应重试）
                if is_permanent_error(&error_msg) {
                    warn!("{}: 永久性错误，不重试: {}", operation_name, error_msg);
                    return Err(e);
                }

                last_error = Some(e);

                if attempt < MAX_RETRIES {
                    let delay = Duration::from_millis(RETRY_BASE_DELAY_MS * 2u64.pow(attempt));
                    warn!(
                        "{}: 第{}次尝试失败，{}ms 后重试: {}",
                        operation_name,
                        attempt + 1,
                        delay.as_millis(),
                        error_msg
                    );
                    std::thread::sleep(delay);
                } else {
                    warn!("{}: 所有{}次重试均失败", operation_name, MAX_RETRIES + 1);
                }
            }
        }
    }

    Err(last_error.unwrap())
}

/// 判断是否为永久性错误（不应重试）
fn is_permanent_error(error_msg: &str) -> bool {
    let msg = error_msg.to_lowercase();

    // 认证错误
    if msg.contains("401") || msg.contains("unauthorized") || msg.contains("invalid api key") {
        return true;
    }

    // 请求格式错误
    if msg.contains("400") || msg.contains("bad request") {
        return true;
    }

    // 资源不存在
    if msg.contains("404") || msg.contains("not found") {
        return true;
    }

    // 无效模型
    if msg.contains("model_not_found") || msg.contains("invalid model") {
        return true;
    }

    false
}

// ─── 聊天消息 ───

/// 对话历史中的聊天消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// ─── SSE 事件 ───

/// 来自 LLM 的单个 SSE 流式分块
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// 增量文本内容（中间分块可能为空）
    pub content: String,
    /// 是否为最终分块
    pub done: bool,
    /// 思考/推理文本（如 DeepSeek R1 的 reasoning_content）。
    /// 仅在模型产生时发出；大多数分块为 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

// ─── RAG 响应（非流式回退）───

/// 带来源的完整 RAG 响应（用于回退模式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGResponse {
    /// AI 生成的答案
    pub answer: String,
    /// 用于上下文的来源分块
    pub sources: Vec<RAGSource>,
    /// LLM 是否可用
    pub llm_available: bool,
}

/// RAG 响应中的来源引用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGSource {
    pub title: String,
    pub section_path: Option<String>,
    pub content_snippet: String,
    pub score: f32,
}

// ─── Token 计数 ───

/// 对记忆搜索结果应用时间衰减。
///
/// 受 OpenClaw 的 temporal-decay.ts 启发 — 较旧的记忆获得指数级较低的有效分数，
/// 因此 top_k 自然过滤掉过时的上下文。
/// 半衰期 = 30 天：30 天后分数减半，60 天后减至四分之一。
fn apply_memory_temporal_decay(
    results: &mut Vec<HybridSearchResult>,
    metadata: &Mutex<MetadataStore>,
) {
    let half_life_days = MEMORY_HALF_LIFE_DAYS;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as f64;

    // 构建 chunk_id → created_at 查找表
    let chunk_ids: Vec<i64> = results.iter().map(|r| r.chunk_id).collect();
    let chunks = metadata
        .lock()
        .ok()
        .and_then(|meta| meta.get_chunks_by_vector_keys(&chunk_ids).ok())
        .unwrap_or_default();
    let created_at_map: std::collections::HashMap<i64, String> =
        chunks.into_iter().map(|c| (c.id, c.created_at)).collect();

    for r in results.iter_mut() {
        if let Some(created_at) = created_at_map.get(&r.chunk_id) {
            // 解析 created_at — 格式："2024-01-15T10:30:00" 或类似的 ISO 格式
            if let Some(age_days) = parse_age_days(created_at, now) {
                let lambda = std::f64::consts::LN_2 / half_life_days;
                let decay = (-lambda * age_days).exp();
                r.score *= decay as f32;
            }
        }
    }

    // 按衰减后的分数重新排序
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// 解析 ISO 风格的日期字符串，返回从 `now_secs` 起的天数。
fn parse_age_days(iso: &str, now_secs: f64) -> Option<f64> {
    // 接受格式："2024-01-15T10:30:00" 或 "2024-01-15 10:30:00"
    let cleaned = iso.trim();
    if cleaned.len() < 10 {
        return None;
    }
    let year: f64 = cleaned[..4].parse().ok()?;
    let month: f64 = cleaned[5..7].parse().ok()?;
    let day: f64 = cleaned[8..10].parse().ok()?;

    // 近似：自 epoch 以来的天数，不精确（对衰减来说足够好）
    let date_days = year * 365.25 + month * 30.44 + day;
    let now_days = now_secs / 86400.0;
    let age = now_days - date_days;
    Some(age.max(0.0))
}
///
/// 如果 tiktoken 失败，回退到粗略的基于字符的估计。
pub fn count_tokens(text: &str) -> u32 {
    match tiktoken_rs::cl100k_base() {
        Ok(bpe) => bpe.encode_with_special_tokens(text).len() as u32,
        Err(_) => {
            // 粗略回退：混合 CJK/英文每 token 约 4 个字符
            (text.chars().count() as f32 / 2.5).ceil() as u32
        }
    }
}

/// 截断文本以适应 token 预算。
///
/// 通过在最后一个有效字符处截断来保留 UTF-8 字符边界。
pub fn truncate_to_tokens(text: &str, max_tokens: u32) -> String {
    let total = count_tokens(text);
    if total <= max_tokens {
        return text.to_string();
    }

    // 二分查找合适的字符数
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

// ─── 上下文组装 ───

/// 将混合搜索结果格式化为 LLM 提示的上下文字符串。
///
/// Format per SPEC.md §5.5:
/// ```text
/// [来源：title | section_path]
/// content
/// ```
pub fn assemble_context(results: &[HybridSearchResult], max_tokens: u32) -> String {
    let mut context = String::new();

    for result in results {
        let section = result.section_path.as_deref().unwrap_or("（无章节信息）");

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
    /// 供应商管理器（获取默认供应商配置）
    providers: Arc<Mutex<LLMProviderManager>>,
    /// HTTP client (reusable for connection pooling)
    client: reqwest::Client,
}

impl LLMService {
    /// Create a new LLM service backed by LLMProviderManager.
    pub fn new(providers: Arc<Mutex<LLMProviderManager>>) -> Self {
        Self {
            providers,
            client: reqwest::Client::new(),
        }
    }

    /// Get the active provider config from the default provider.
    pub fn get_active_config(&self) -> Result<LLMProviderConfig, String> {
        let mgr = self.providers.lock().map_err(|e| e.to_string())?;
        mgr.get_default_provider()
            .cloned()
            .ok_or_else(|| "未配置默认 LLM 供应商".to_string())
    }

    /// Get config for a specific provider by ID, falling back to default if not found.
    pub fn get_config_for_provider(&self, provider_id: Option<&str>) -> Result<LLMProviderConfig, String> {
        match provider_id {
            Some(id) => {
                let mgr = self.providers.lock().map_err(|e| e.to_string())?;
                mgr.get_provider(id)
                    .cloned()
                    .ok_or_else(|| format!("供应商 '{}' 不存在", id))
            }
            None => self.get_active_config(),
        }
    }

    /// Synchronous text generation (non-streaming) for internal backend use.
    ///
    /// Uses `ureq` for a simple blocking HTTP call. Returns the complete generated text.
    /// Includes exponential backoff retry for transient errors.
    pub fn generate_text_sync(
        &self,
        system_prompt: &str,
        user_message: &str,
    ) -> Result<String, String> {
        let config = self.get_active_config()?;

        if config.api_key.is_empty() {
            return Err("LLM API key not configured".to_string());
        }

        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let auth_header = format!("Bearer {}", config.api_key);
        let model = config.model.clone();
        let temperature = config.temperature;
        let max_tokens = config.max_tokens;

        with_retry_sync("LLM 生成", || {
            let body = serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": system_prompt },
                    { "role": "user", "content": user_message }
                ],
                "temperature": temperature,
                "max_tokens": max_tokens,
                "stream": false
            });

            let response: serde_json::Value = ureq::post(&url)
                .header("Authorization", &auth_header)
                .header("Content-Type", "application/json")
                .send_json(&body)
                .map_err(|e| format!("LLM request failed: {}", e))?
                .body_mut()
                .read_json()
                .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

            let text = response["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();

            Ok(text)
        })
    }

    fn needs_relaxed_tls(base_url: &str) -> bool {
        let base_url = base_url.to_ascii_lowercase();
        base_url.starts_with("https://maas.gd.chinamobile.com")
    }

    fn client_for_config(&self, config: &LLMProviderConfig) -> Result<reqwest::Client, String> {
        if Self::needs_relaxed_tls(&config.base_url) {
            return reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .http1_only()
                .no_proxy()
                .build()
                .map_err(|e| format!("Build HTTP client failed: {}", e));
        }

        Ok(self.client.clone())
    }

    /// Check if the LLM is configured (has API key, or is a local model).
    pub fn is_configured(&self) -> bool {
        self.get_active_config()
            .map(|cfg| cfg.is_configured())
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

        // Step 3: Read config from provider manager
        let config = self.get_active_config()?;

        // Step 4: Compress conversation history if it exceeds token threshold
        // (OpenCode-inspired: summarize older turns, keep last 2 pairs verbatim)
        let compressed = self.compress_conversation(&conversation_history).await;
        let compressed_history = match compressed {
            Ok(c) => c,
            Err(_) => conversation_history,
        };

        // Step 5: Assemble context
        let system_tokens = count_tokens(SYSTEM_PROMPT);
        let budget = config
            .max_tokens
            .saturating_sub(system_tokens + RESPONSE_TOKENS + 200);
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
        match config.protocol {
            LLMProtocol::OpenAI | LLMProtocol::Local => {
                self.rag_query_openai(&config, SYSTEM_PROMPT, &messages)
                    .await
            }
            LLMProtocol::Anthropic => {
                self.rag_query_anthropic(&config, SYSTEM_PROMPT, &messages)
                    .await
            }
        }
    }

    /// OpenAI streaming RAG query — POST /chat/completions with OpenAI SSE format
    ///
    /// Uses `reqwest_eventsource::EventSource` for robust SSE parsing.
    async fn rag_query_openai(
        &self,
        config: &LLMProviderConfig,
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
            .client_for_config(config)?
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create EventSource: {}", e))?;

        let mut chunks = Vec::new();

        // Wait for the first event with timeout (connection + first data chunk)
        let first_event = tokio::time::timeout(
            Duration::from_secs(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS),
            es.next(),
        )
        .await
        .map_err(|_| "LLM 流式响应超时，未收到首个数据块".to_string())?;

        // Process first event if received
        if let Some(event) = first_event {
            match event {
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        chunks.push(StreamChunk {
                            content: String::new(),
                            done: true,
                            thinking: None,
                        });
                        return Ok(chunks);
                    }
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&msg.data) {
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
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            let cleaned = scrub_response(content);
                            if !cleaned.is_empty() {
                                chunks.push(StreamChunk {
                                    content: cleaned,
                                    done: false,
                                    thinking: None,
                                });
                            }
                        }
                        if let Some(reason) = parsed["choices"][0]["finish_reason"].as_str() {
                            if !reason.is_empty() && reason != "null" {
                                chunks.push(StreamChunk {
                                    content: String::new(),
                                    done: true,
                                    thinking: None,
                                });
                                return Ok(chunks);
                            }
                        }
                    }
                }
                Ok(Event::Open) => {}
                Err(e) => return Err(format!("SSE error: {}", e)),
            }
        }

        // Process remaining events (first-chunk timeout already passed)
        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        chunks.push(StreamChunk {
                            content: String::new(),
                            done: true,
                            thinking: None,
                        });
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
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            let cleaned = scrub_response(content);
                            if !cleaned.is_empty() {
                                chunks.push(StreamChunk {
                                    content: cleaned,
                                    done: false,
                                    thinking: None,
                                });
                            }
                        }
                        // Check finish_reason
                        if let Some(reason) = parsed["choices"][0]["finish_reason"].as_str() {
                            if !reason.is_empty() && reason != "null" {
                                chunks.push(StreamChunk {
                                    content: String::new(),
                                    done: true,
                                    thinking: None,
                                });
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
                        chunks.push(StreamChunk {
                            content: String::new(),
                            done: true,
                            thinking: None,
                        });
                        return Ok(chunks);
                    }
                    return Err(format!("SSE error: {}", e));
                }
            }
        }

        if chunks.last().map(|c| c.done) != Some(true) {
            chunks.push(StreamChunk {
                content: String::new(),
                done: true,
                thinking: None,
            });
        }
        Ok(chunks)
    }

    /// Anthropic streaming RAG query — POST /messages with Anthropic SSE format
    ///
    /// Uses `reqwest_eventsource::EventSource` for robust SSE parsing.
    /// Anthropic sends events: `content_block_delta`, `message_delta`, `message_stop`.
    async fn rag_query_anthropic(
        &self,
        config: &LLMProviderConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
    ) -> Result<Vec<StreamChunk>, String> {
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));

        // Anthropic: system prompt is a top-level field, NOT in messages array
        // Filter out any system messages from the messages array
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content
                })
            })
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

        // Wait for the first event with timeout (connection + first data chunk)
        let first_event = tokio::time::timeout(
            Duration::from_secs(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS),
            es.next(),
        )
        .await
        .map_err(|_| "LLM 流式响应超时，未收到首个数据块".to_string())?;

        // Process first event if received
        if let Some(event) = first_event {
            match event {
                Ok(Event::Message(msg)) => match msg.event.as_str() {
                    "content_block_delta" => {
                        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                            if data["type"] == "content_block_delta" {
                                if let Some(delta) = data.get("delta") {
                                    if delta["type"] == "text_delta" {
                                        if let Some(text) = delta["text"].as_str() {
                                            let cleaned = scrub_response(text);
                                            if !cleaned.is_empty() {
                                                chunks.push(StreamChunk {
                                                    content: cleaned,
                                                    done: false,
                                                    thinking: None,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "message_delta" | "message_stop" => {
                        chunks.push(StreamChunk {
                            content: String::new(),
                            done: true,
                            thinking: None,
                        });
                        return Ok(chunks);
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
                },
                Ok(Event::Open) => {}
                Err(e) => return Err(format!("SSE error: {}", e)),
            }
        }

        // Process remaining events (first-chunk timeout already passed)
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
                                                    chunks.push(StreamChunk {
                                                        content: cleaned,
                                                        done: false,
                                                        thinking: None,
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "message_delta" => {
                            chunks.push(StreamChunk {
                                content: String::new(),
                                done: true,
                                thinking: None,
                            });
                            return Ok(chunks);
                        }
                        "message_stop" => {
                            // Safety net: some Anthropic-compatible endpoints
                            // skip message_delta — ensure done is emitted
                            chunks.push(StreamChunk {
                                content: String::new(),
                                done: true,
                                thinking: None,
                            });
                            return Ok(chunks);
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
                Ok(Event::Open) => {
                    // Connection established — no action needed
                }
                Err(e) => {
                    // If we already have content, return it gracefully
                    if !chunks.is_empty() {
                        chunks.push(StreamChunk {
                            content: String::new(),
                            done: true,
                            thinking: None,
                        });
                        return Ok(chunks);
                    }
                    return Err(format!("SSE error: {}", e));
                }
            }
        }

        if chunks.last().map(|c| c.done) != Some(true) {
            chunks.push(StreamChunk {
                content: String::new(),
                done: true,
                thinking: None,
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
    ///
    /// Branches by provider: OpenAI uses /chat/completions, Anthropic uses /messages.
    pub async fn chat_completion(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        if config.api_key.is_empty() {
            return Err("LLM API key not configured".to_string());
        }

        match config.protocol {
            LLMProtocol::OpenAI | LLMProtocol::Local => {
                let result = self
                    .chat_completion_openai_with_tools(messages, config, &[], true)
                    .await?;
                Ok(result)
            }
            LLMProtocol::Anthropic => self.chat_completion_anthropic(messages, config).await,
        }
    }

    /// Chat completion with OpenAI-style function calling support.
    /// Returns the raw content string (strips tool_calls).
    /// If `tools` is non-empty, sends with `tool_choice: "auto"`.
    async fn chat_completion_openai_with_tools(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
        tools: &[serde_json::Value],
        _stream: bool,
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

        let mut body = serde_json::json!({
            "model": config.model,
            "messages": api_messages,
            "temperature": config.temperature,
            "stream": false
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
            body["tool_choice"] = serde_json::json!("auto");
        }

        let request_future = async {
            let response = self
                .client_for_config(config)?
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
        };

        tokio::time::timeout(
            Duration::from_secs(LLM_CALL_TIMEOUT_SECS),
            request_future,
        )
        .await
        .map_err(|_| "LLM 调用超时，请检查网络连接或稍后重试".to_string())?
    }

    /// Anthropic non-streaming chat completion — POST /messages
    ///
    /// Anthropic requires `system` as a top-level field, not in messages.
    /// Response format: `{"content":[{"type":"text","text":"..."}]}`
    async fn chat_completion_anthropic(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
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
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content
                })
            })
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

        let request_future = async {
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
        };

        tokio::time::timeout(
            Duration::from_secs(LLM_CALL_TIMEOUT_SECS),
            request_future,
        )
        .await
        .map_err(|_| "LLM 调用超时，请检查网络连接或稍后重试".to_string())?
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
        let config = self.get_active_config()?;

        // Compress conversation if too long (OpenCode-inspired)
        let compressed = self.compress_conversation(&conversation_history).await;
        let conversation_history = match compressed {
            Ok(c) => c,
            Err(_) => conversation_history,
        };

        // Step 4: Assemble context
        let system_tokens = count_tokens(SYSTEM_PROMPT);
        let budget = config
            .max_tokens
            .saturating_sub(system_tokens + RESPONSE_TOKENS + 200);
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
        match config.protocol {
            LLMProtocol::OpenAI | LLMProtocol::Local => {
                self.stream_openai_to_sender(&config, SYSTEM_PROMPT, &messages, &tx)
                    .await
            }
            LLMProtocol::Anthropic => {
                self.stream_anthropic_to_sender(&config, SYSTEM_PROMPT, &messages, &tx)
                    .await
            }
        }
    }

    /// Stream OpenAI response to channel sender (real-time).
    async fn stream_openai_to_sender(
        &self,
        config: &LLMProviderConfig,
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
            .client_for_config(config)?
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create EventSource: {}", e))?;

        // Wait for the first event with timeout
        let first_event = tokio::time::timeout(
            Duration::from_secs(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS),
            es.next(),
        )
        .await
        .map_err(|_| "LLM 流式响应超时，未收到首个数据块".to_string())?;

        // Process first event if received
        if let Some(event) = first_event {
            match event {
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        let _ = tx
                            .send(StreamChunk {
                                content: String::new(),
                                done: true,
                                thinking: None,
                            })
                            .await;
                        return Ok(());
                    }
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                        if let Some(reasoning) =
                            parsed["choices"][0]["delta"]["reasoning_content"].as_str()
                        {
                            if !reasoning.is_empty() {
                                let _ = tx
                                    .send(StreamChunk {
                                        content: String::new(),
                                        done: false,
                                        thinking: Some(reasoning.to_string()),
                                    })
                                    .await;
                            }
                        }
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            let cleaned = scrub_response(content);
                            if !cleaned.is_empty() {
                                let _ = tx
                                    .send(StreamChunk {
                                        content: cleaned,
                                        done: false,
                                        thinking: None,
                                    })
                                    .await;
                            }
                        }
                        if let Some(reason) = parsed["choices"][0]["finish_reason"].as_str() {
                            if !reason.is_empty() && reason != "null" {
                                let _ = tx
                                    .send(StreamChunk {
                                        content: String::new(),
                                        done: true,
                                        thinking: None,
                                    })
                                    .await;
                                return Ok(());
                            }
                        }
                    }
                }
                Ok(Event::Open) => {}
                Err(e) => return Err(format!("SSE error: {}", e)),
            }
        }

        // Process remaining events (first-chunk timeout already passed)
        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    if msg.data == "[DONE]" {
                        let _ = tx
                            .send(StreamChunk {
                                content: String::new(),
                                done: true,
                                thinking: None,
                            })
                            .await;
                        return Ok(());
                    }

                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                        // Extract reasoning/thinking content (e.g. DeepSeek R1)
                        if let Some(reasoning) =
                            parsed["choices"][0]["delta"]["reasoning_content"].as_str()
                        {
                            if !reasoning.is_empty() {
                                let _ = tx
                                    .send(StreamChunk {
                                        content: String::new(),
                                        done: false,
                                        thinking: Some(reasoning.to_string()),
                                    })
                                    .await;
                            }
                        }
                        // Extract visible text content
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            let cleaned = scrub_response(content);
                            if !cleaned.is_empty() {
                                let _ = tx
                                    .send(StreamChunk {
                                        content: cleaned,
                                        done: false,
                                        thinking: None,
                                    })
                                    .await;
                            }
                        }
                        if let Some(reason) = parsed["choices"][0]["finish_reason"].as_str() {
                            if !reason.is_empty() && reason != "null" {
                                let _ = tx
                                    .send(StreamChunk {
                                        content: String::new(),
                                        done: true,
                                        thinking: None,
                                    })
                                    .await;
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
        let _ = tx
            .send(StreamChunk {
                content: String::new(),
                done: true,
                thinking: None,
            })
            .await;
        Ok(())
    }

    /// Stream Anthropic response to channel sender (real-time).
    async fn stream_anthropic_to_sender(
        &self,
        config: &LLMProviderConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
        tx: &mpsc::Sender<StreamChunk>,
    ) -> Result<(), String> {
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));

        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content
                })
            })
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
            .client_for_config(config)?
            .post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("Content-Type", "application/json")
            .json(&body);

        let mut es = EventSource::new(request)
            .map_err(|e| format!("Failed to create EventSource: {}", e))?;

        // Wait for the first event with timeout
        let first_event = tokio::time::timeout(
            Duration::from_secs(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS),
            es.next(),
        )
        .await
        .map_err(|_| "LLM 流式响应超时，未收到首个数据块".to_string())?;

        // Process first event if received
        if let Some(event) = first_event {
            match event {
                Ok(Event::Message(msg)) => match msg.event.as_str() {
                    "content_block_delta" => {
                        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                            if data["type"] == "content_block_delta" {
                                if let Some(delta) = data.get("delta") {
                                    if delta["type"] == "text_delta" {
                                        if let Some(text) = delta["text"].as_str() {
                                            let cleaned = scrub_response(text);
                                            if !cleaned.is_empty() {
                                                let _ = tx
                                                    .send(StreamChunk {
                                                        content: cleaned,
                                                        done: false,
                                                        thinking: None,
                                                    })
                                                    .await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "message_delta" | "message_stop" => {
                        let _ = tx
                            .send(StreamChunk {
                                content: String::new(),
                                done: true,
                                thinking: None,
                            })
                            .await;
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
                },
                Ok(Event::Open) => {}
                Err(e) => return Err(format!("SSE error: {}", e)),
            }
        }

        // Process remaining events (first-chunk timeout already passed)
        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Message(msg)) => match msg.event.as_str() {
                    "content_block_delta" => {
                        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                            if data["type"] == "content_block_delta" {
                                if let Some(delta) = data.get("delta") {
                                    if delta["type"] == "text_delta" {
                                        if let Some(text) = delta["text"].as_str() {
                                            let cleaned = scrub_response(text);
                                            if !cleaned.is_empty() {
                                                let _ = tx
                                                    .send(StreamChunk {
                                                        content: cleaned,
                                                        done: false,
                                                        thinking: None,
                                                    })
                                                    .await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "message_delta" => {
                        let _ = tx
                            .send(StreamChunk {
                                content: String::new(),
                                done: true,
                                thinking: None,
                            })
                            .await;
                        return Ok(());
                    }
                    "message_stop" => {
                        let _ = tx
                            .send(StreamChunk {
                                content: String::new(),
                                done: true,
                                thinking: None,
                            })
                            .await;
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
                },
                Ok(Event::Open) => {}
                Err(e) => {
                    return Err(format!("SSE error: {}", e));
                }
            }
        }

        // Stream ended without done marker
        let _ = tx
            .send(StreamChunk {
                content: String::new(),
                done: true,
                thinking: None,
            })
            .await;
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
        let total_tokens = estimate_tokens(conversation);

        if total_tokens <= COMPRESS_THRESHOLD || !self.is_configured() {
            return Ok(conversation.to_vec());
        }

        info!(
            target: "llm",
            total_tokens,
            threshold = COMPRESS_THRESHOLD,
            "[Compress] Conversation tokens exceed threshold — starting summarization"
        );

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

        let config = self.get_active_config()?;

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
                content:
                    "你是一个对话摘要助手。提取关键信息，保持项目名、决策、技术选型、业务规则等。\
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
                let compressed_tokens = estimate_tokens(&result);
                info!(
                    target: "llm",
                    head_count = head.len(),
                    compressed_tokens,
                    total_tokens,
                    tail_count = tail.len(),
                    "[Compress] Summarized head messages"
                );
                Ok(result)
            }
            Err(e) => {
                warn!(
                    target: "llm",
                    error = %e,
                    "[Compress] LLM summarization failed — keeping full history"
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
        let config = self.get_active_config()?;
        // Local models (non-standard) are allowed to have empty API key
        let is_local = config.protocol == LLMProtocol::Local;
        if config.api_key.is_empty() && !is_local {
            return Err("API Key 未配置".to_string());
        }

        match config.protocol {
            LLMProtocol::OpenAI | LLMProtocol::Local => {
                let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": config.model,
                    "messages": [{"role": "user", "content": "Hi"}],
                    "max_tokens": 5,
                    "temperature": 0.0
                });

                let response = tokio::time::timeout(
                    Duration::from_secs(LLM_CALL_TIMEOUT_SECS),
                    self.client
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", config.api_key))
                        .header("Content-Type", "application/json")
                        .json(&body)
                        .send(),
                )
                .await
                .map_err(|_| "LLM 连接测试超时".to_string())?
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
            LLMProtocol::Anthropic => {
                let url = format!("{}/messages", config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": config.model,
                    "max_tokens": 5,
                    "temperature": 0.0,
                    "messages": [{"role": "user", "content": "Hi"}]
                });

                let response = tokio::time::timeout(
                    Duration::from_secs(LLM_CALL_TIMEOUT_SECS),
                    self.client_for_config(&config)?
                        .post(&url)
                        .header("x-api-key", &config.api_key)
                        .header("anthropic-version", ANTHROPIC_VERSION)
                        .header("anthropic-dangerous-direct-browser-access", "true")
                        .header("Content-Type", "application/json")
                        .json(&body)
                        .send(),
                )
                .await
                .map_err(|_| "LLM 连接测试超时".to_string())?
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
            let section = r.section_path.as_deref().unwrap_or("（无章节信息）");
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
