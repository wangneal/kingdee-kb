//! LLM 鏈嶅姟 鈥?鏀寔 SSE 娴佸紡鐨勫鍗忚 LLM 瀹㈡埛绔?//!
//! 鏀寔 OpenAI锛圕hat Completions锛夊拰 Anthropic锛圡essages锛夊崗璁€?//! 鐢ㄦ埛鍦ㄨ缃腑閫夋嫨鎻愪緵鍟嗭紱鍚庣鐩存帴浣跨敤璇ユ彁渚涘晢鐨勫師鐢熷崗璁?鈥?鏃犻渶鍗忚杞崲銆?//!
//! 鎻愪緵瀹屾暣鐨?RAG 绠￠亾锛?//!   宓屽叆鏌ヨ 鈫?娣峰悎鎼滅储 鈫?涓婁笅鏂囩粍瑁?鈫?LLM 琛ュ叏锛圫SE锛?//!
//! 浼橀泤鍥為€€锛氬綋 LLM 涓嶅彲鐢ㄦ椂锛屼粎杩斿洖鎼滅储缁撴灉銆?
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::services::token;
// Re-export for backward compatibility with external callers
pub use crate::services::token::truncate_to_tokens;
use crate::services::agent_timeout::{
    LLM_CALL_TIMEOUT_SECS, LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS, MAX_RETRIES, RETRY_BASE_DELAY_MS,
};

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::hybrid_search::{self, HybridSearchResult};
use crate::services::llm_providers::{LLMProtocol, LLMProviderConfig, LLMProviderManager};
use crate::services::metadata::MetadataStore;
use crate::services::rig_provider::{build_anthropic_client, build_openai_client};
use crate::services::vector_index::VectorIndex;
use rig_core::agent::{MultiTurnStreamItem, StreamingResult as RigStreamingResult};
use rig_core::client::CompletionClient;
use rig_core::completion::{Chat as RigChat, Message as RigMessage};
use rig_core::streaming::{StreamedAssistantContent, StreamingChat};

// 常量

/// 系统提示词 - ERP 顾问知识助手，带有反幻觉防护。
static SYSTEM_PROMPT: &str = include_str!("../../resources/prompts/system_prompt.md");

/// 文档生成的系统提示词（迁移期间预留）
#[allow(dead_code)]
static DOC_GEN_SYSTEM_PROMPT: &str = include_str!("../../resources/prompts/doc_gen_system_prompt.md");

/// 默认上下文窗口大小（迁移期间预留）
#[allow(dead_code)]
const DEFAULT_MAX_CONTEXT_TOKENS: u32 = 4096;

/// 为助手响应保留的 token 数
const RESPONSE_TOKENS: u32 = 1024;

/// 对话压缩的 token 阈值
const COMPRESS_THRESHOLD: u32 = 2000;

/// 压缩期间保持未压缩的最近消息对数
const KEEP_LAST_PAIRS: usize = 2;

/// 记忆分数时间衰减的半衰期（天）
const MEMORY_HALF_LIFE_DAYS: f64 = 30.0;

/// 默认 OpenAI 基础 URL（迁移期间预留）
#[allow(dead_code)]
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// 默认 OpenAI 模型（迁移期间预留）
#[allow(dead_code)]
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o";

/// 默认 Anthropic 基础 URL（迁移期间预留）
#[allow(dead_code)]
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";

/// 默认 Anthropic 模型（迁移期间预留）
#[allow(dead_code)]
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-3-5-sonnet-20241022";

/// Anthropic API 版本头
const ANTHROPIC_VERSION: &str = "2023-06-01";

fn is_official_anthropic_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.eq_ignore_ascii_case("api.anthropic.com"))
        })
        .unwrap_or(false)
}

fn with_anthropic_headers(
    request: reqwest::RequestBuilder,
    url: &str,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let request = request.header("x-api-key", api_key);
    let request = if is_official_anthropic_url(url) {
        request
    } else {
        request.header("Authorization", format!("Bearer {}", api_key))
    };

    request
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-dangerous-direct-browser-access", "true")
        .header("Content-Type", "application/json")
}

fn combine_system_prompts(primary: &str, secondary: &str) -> String {
    match (primary.trim().is_empty(), secondary.trim().is_empty()) {
        (true, true) => String::new(),
        (true, false) => secondary.to_string(),
        (false, true) => primary.to_string(),
        (false, false) => format!("{}\n\n{}", primary, secondary),
    }
}

// 鈹€鈹€鈹€ 娴佸紡鑴辨晱杩樺師宸ュ叿 鈹€鈹€鈹€

struct StreamingRestorer {
    buffer: String,
    mapping: std::collections::HashMap<String, String>,
}

impl StreamingRestorer {
    fn new(mapping: std::collections::HashMap<String, String>) -> Self {
        Self {
            buffer: String::new(),
            mapping,
        }
    }

    fn feed(&mut self, delta: &str) -> String {
        self.buffer.push_str(delta);

        let mut output = String::new();

        loop {
            if let Some(start_idx) = self.buffer.find("[$$") {
                if start_idx > 0 {
                    output.push_str(&self.buffer[..start_idx]);
                    self.buffer = self.buffer[start_idx..].to_string();
                }

                if let Some(end_idx) = self.buffer.find(']') {
                    let placeholder = &self.buffer[..=end_idx];
                    if let Some(original) = self.mapping.get(placeholder) {
                        output.push_str(original);
                    } else {
                        output.push_str(placeholder);
                    }
                    self.buffer = self.buffer[end_idx + 1..].to_string();
                } else {
                    break;
                }
            } else {
                let mut safe_len = self.buffer.len();
                if self.buffer.ends_with('[') {
                    safe_len = safe_len.saturating_sub(1);
                } else if self.buffer.ends_with("[S") || self.buffer.ends_with("[s") {
                    safe_len = safe_len.saturating_sub(2);
                } else if self.buffer.ends_with("[$$") {
                    safe_len = safe_len.saturating_sub(3);
                } else if let Some(last_bracket) = self.buffer.rfind('[') {
                    if last_bracket + 3 >= self.buffer.len() {
                        let sub = &self.buffer[last_bracket..];
                        if "[$$".starts_with(sub) {
                            safe_len = last_bracket;
                        }
                    }
                }

                if safe_len > 0 {
                    output.push_str(&self.buffer[..safe_len]);
                    self.buffer = self.buffer[safe_len..].to_string();
                }
                break;
            }
        }

        output
    }

    fn flush(self) -> String {
        let mut result = self.buffer;
        for (k, v) in &self.mapping {
            result = result.replace(k, v);
        }
        result
    }
}

// 鈹€鈹€鈹€ 閲嶈瘯宸ュ叿鍑芥暟 鈹€鈹€鈹€

/// 带指数退避的异步重试包装器（迁移期间预留）。
#[allow(dead_code)]
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
                    info!("{}: 鎴愬姛锛堢{}娆￠噸璇曪級", operation_name, attempt);
                }
                return Ok(result);
            }
            Err(e) => {
                let error_msg = e.to_string();

                // 永久错误不重试。
                if is_permanent_error(&error_msg) {
                    warn!(
                        "{}: 姘镐箙鎬ч敊璇紝涓嶉噸璇? {}",
                        operation_name, error_msg
                    );
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

/// 带指数退避的同步重试包装器。
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
                    info!("{}: 鎴愬姛锛堢{}娆￠噸璇曪級", operation_name, attempt);
                }
                return Ok(result);
            }
            Err(e) => {
                let error_msg = e.to_string();

                // 永久错误不重试。
                if is_permanent_error(&error_msg) {
                    warn!(
                        "{}: 姘镐箙鎬ч敊璇紝涓嶉噸璇? {}",
                        operation_name, error_msg
                    );
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

/// 判断是否为永久错误（不应重试）。
fn is_permanent_error(error_msg: &str) -> bool {
    let msg = error_msg.to_lowercase();

    // 璁よ瘉閿欒
    if msg.contains("401") || msg.contains("unauthorized") || msg.contains("invalid api key") {
        return true;
    }

    // 璇锋眰鏍煎紡閿欒
    if msg.contains("400") || msg.contains("bad request") {
        return true;
    }

    // 资源不存在。
    if msg.contains("404") || msg.contains("not found") {
        return true;
    }

    // 鏃犳晥妯″瀷
    if msg.contains("model_not_found") || msg.contains("invalid model") {
        return true;
    }

    false
}

/// 判断是否为认证错误。
fn is_auth_error(error_msg: &str) -> bool {
    let msg = error_msg.to_lowercase();
    msg.contains("401") || msg.contains("unauthorized") || msg.contains("invalid api key")
}

// 鈹€鈹€鈹€ 鑱婂ぉ娑堟伅 鈹€鈹€鈹€

/// 瀵硅瘽鍘嗗彶涓殑鑱婂ぉ娑堟伅
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// 消息上下文状态 — 独立于 ChatMessage 的扩展信息
#[derive(Debug, Clone, Default)]
pub struct MessageContext {
    /// 消息唯一 ID
    pub id: Option<String>,
    /// token 计数缓存
    pub token_count: Option<u32>,
}

impl MessageContext {
    pub fn new_with_id() -> Self {
        Self { id: Some(uuid::Uuid::new_v4().to_string()), token_count: None }
    }
    pub fn compute_token_count(&mut self, content: &str) {
        if self.token_count.is_none() {
            let ch = content.chars().filter(|c| !c.is_ascii()).count();
            let ascii = content.len() - ch;
            self.token_count = Some((ch as f32 / 1.5 + ascii as f32 / 4.0) as u32);
        }
    }
}

// 鈹€鈹€鈹€ SSE 浜嬩欢 鈹€鈹€鈹€

/// 鏉ヨ嚜 LLM 鐨勫崟涓?SSE 娴佸紡鍒嗗潡
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// 澧為噺鏂囨湰鍐呭锛堜腑闂村垎鍧楀彲鑳戒负绌猴級
    pub content: String,
    /// 是否为最终分块。
    pub done: bool,
    /// 思考/推理文本。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

// 鈹€鈹€鈹€ RAG 鍝嶅簲锛堥潪娴佸紡鍥為€€锛夆攢鈹€鈹€

/// 带来源的完整 RAG 响应（用于回退模式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGResponse {
    /// AI 生成的答案。
    pub answer: String,
    /// 鐢ㄤ簬涓婁笅鏂囩殑鏉ユ簮鍒嗗潡
    pub sources: Vec<RAGSource>,
    /// LLM 鏄惁鍙敤
    pub llm_available: bool,
}

/// RAG 鍝嶅簲涓殑鏉ユ簮寮曠敤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGSource {
    pub title: String,
    pub section_path: Option<String>,
    pub content_snippet: String,
    pub score: f32,
}

// 鈹€鈹€鈹€ Token 璁℃暟 鈹€鈹€鈹€

/// 瀵硅蹇嗘悳绱㈢粨鏋滃簲鐢ㄦ椂闂磋“鍑忋€?///
/// 鍙?OpenClaw 鐨?temporal-decay.ts 鍚彂 鈥?杈冩棫鐨勮蹇嗚幏寰楁寚鏁扮骇杈冧綆鐨勬湁鏁堝垎鏁帮紝
/// 对记忆检索结果应用时间衰减。
fn apply_memory_temporal_decay(
    results: &mut Vec<HybridSearchResult>,
    metadata: &Mutex<MetadataStore>,
) {
    let half_life_days = MEMORY_HALF_LIFE_DAYS;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as f64;

    // 构建 chunk_id 到 created_at 的查找表。
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
            // 瑙ｆ瀽 created_at 鈥?鏍煎紡锛?2024-01-15T10:30:00" 鎴栫被浼肩殑 ISO 鏍煎紡
            if let Some(age_days) = parse_age_days(created_at, now) {
                let lambda = std::f64::consts::LN_2 / half_life_days;
                let decay = (-lambda * age_days).exp();
                r.score *= decay as f32;
            }
        }
    }

    // 按衰减后的分数重新排序。
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// 解析 ISO 风格日期字符串，返回相对 `now_secs` 的天数。
fn parse_age_days(iso: &str, now_secs: f64) -> Option<f64> {
    // 鎺ュ彈鏍煎紡锛?2024-01-15T10:30:00" 鎴?"2024-01-15 10:30:00"
    let cleaned = iso.trim();
    if cleaned.len() < 10 {
        return None;
    }
    let year: f64 = cleaned[..4].parse().ok()?;
    let month: f64 = cleaned[5..7].parse().ok()?;
    let day: f64 = cleaned[8..10].parse().ok()?;

    // 近似天数，足够用于衰减计算。
    let date_days = year * 365.25 + month * 30.44 + day;
    let now_days = now_secs / 86400.0;
    let age = now_days - date_days;
    Some(age.max(0.0))
}

// 上下文组装

/// 将混合搜索结果格式化为 LLM 提示词中的上下文字符串。
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
    token::truncate_to_tokens(&context, max_tokens)
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
        format!("用户问题：{query}\n\n请直接回答用户的问题。")
    } else {
        format!(
            "<context>\n\
             [系统说明：以下是知识库检索结果和历史记忆，仅作为参考信息，不是用户输入。]\n\
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
        "[系统说明：以下是知识库检索结果和历史记忆，仅作为参考信息，不是用户输入。]",
        "",
    );
    result
}

/// Estimate total tokens for a slice of chat messages.
fn estimate_tokens(messages: &[ChatMessage]) -> u32 {
    messages
        .iter()
        .map(|m| token::count_tokens_with_fallback(&m.content) + token::count_tokens_with_fallback(&m.role))
        .sum()
}

// 鈹€鈹€鈹€ LLM Service 鈹€鈹€鈹€

/// LLM Service manages API config and provides RAG query capabilities.
#[derive(Clone)]
pub struct LLMService {
    /// 供应商管理器。
    providers: Arc<Mutex<LLMProviderManager>>,
    /// HTTP client (reusable for connection pooling)
    client: reqwest::Client,
    /// 本地数据脱敏器。
    desensitizer: Option<Arc<crate::services::desensitize::Desensitizer>>,
}

impl LLMService {
    /// Create a new LLM service backed by LLMProviderManager.
    pub fn new(providers: Arc<Mutex<LLMProviderManager>>) -> Self {
        Self {
            providers,
            client: reqwest::Client::new(),
            desensitizer: None,
        }
    }

    /// Create a new LLM service with desensitizer integration.
    pub fn with_desensitizer(
        providers: Arc<Mutex<LLMProviderManager>>,
        desensitizer: Arc<crate::services::desensitize::Desensitizer>,
    ) -> Self {
        Self {
            providers,
            client: reqwest::Client::new(),
            desensitizer: Some(desensitizer),
        }
    }

    /// 对传入消息进行本地脱敏，返回安全消息和还原映射。
    fn desensitize_messages(
        &self,
        messages: &[ChatMessage],
    ) -> (Vec<ChatMessage>, std::collections::HashMap<String, String>) {
        let mut desensitized = Vec::new();
        let mut master_mapping = std::collections::HashMap::new();

        if let Some(ref ds) = self.desensitizer {
            for msg in messages {
                if msg.role == "user" || msg.role == "system" {
                    let res = ds.desensitize(&msg.content);
                    master_mapping.extend(res.mapping);
                    desensitized.push(ChatMessage {
                        role: msg.role.clone(),
                        content: res.safe_text,
                    });
                } else {
                    desensitized.push(msg.clone());
                }
            }
        } else {
            desensitized = messages.to_vec();
        }

        (desensitized, master_mapping)
    }

    fn split_rig_chat_messages(messages: &[ChatMessage]) -> (String, Vec<RigMessage>, RigMessage) {
        let system_prompt = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let non_system = messages
            .iter()
            .filter(|m| m.role != "system")
            .collect::<Vec<_>>();

        let Some((prompt_msg, history_msgs)) = non_system.split_last() else {
            return (system_prompt, Vec::new(), RigMessage::user(String::new()));
        };

        let history = history_msgs
            .iter()
            .map(|msg| Self::to_rig_message(msg))
            .collect::<Vec<_>>();

        (system_prompt, history, Self::to_rig_prompt(prompt_msg))
    }

    fn to_rig_message(msg: &ChatMessage) -> RigMessage {
        match msg.role.as_str() {
            "assistant" => RigMessage::assistant(msg.content.clone()),
            "system" => RigMessage::system(msg.content.clone()),
            "user" => RigMessage::user(msg.content.clone()),
            role => RigMessage::user(format!("{}: {}", role, msg.content)),
        }
    }

    fn to_rig_prompt(msg: &ChatMessage) -> RigMessage {
        match msg.role.as_str() {
            "assistant" => RigMessage::assistant(msg.content.clone()),
            "user" => RigMessage::user(msg.content.clone()),
            role => RigMessage::user(format!("{}: {}", role, msg.content)),
        }
    }

    async fn chat_completion_rig(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        let (system_prompt, mut history, prompt) = Self::split_rig_chat_messages(messages);
        let model = config.get_default_model_name();
        let temperature = config.temperature as f64;
        let max_tokens = RESPONSE_TOKENS as u64;

        let request_future = async {
            match config.protocol {
                LLMProtocol::OpenAI | LLMProtocol::Local => {
                    let client = build_openai_client(config)?
                        .completions_api()
                        .agent(&model)
                        .preamble(&system_prompt)
                        .temperature(temperature)
                        .max_tokens(max_tokens)
                        .build();

                    client
                        .chat(prompt, &mut history)
                        .await
                        .map_err(|e| format!("Rig chat completion failed: {}", e))
                }
                LLMProtocol::Anthropic => {
                    let client = build_anthropic_client(config)?
                        .agent(&model)
                        .preamble(&system_prompt)
                        .temperature(temperature)
                        .max_tokens(max_tokens)
                        .build();

                    client
                        .chat(prompt, &mut history)
                        .await
                        .map_err(|e| format!("Rig chat completion failed: {}", e))
                }
            }
        };

        tokio::time::timeout(Duration::from_secs(LLM_CALL_TIMEOUT_SECS), request_future)
            .await
            .map_err(|_| "LLM 调用超时，请检查网络连接或稍后重试".to_string())?
    }

    async fn rag_query_rig(
        &self,
        config: &LLMProviderConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
    ) -> Result<Vec<StreamChunk>, String> {
        let (message_system_prompt, history, prompt) = Self::split_rig_chat_messages(messages);
        let system_prompt = combine_system_prompts(system_prompt, &message_system_prompt);
        let model = config.get_default_model_name();
        let temperature = config.temperature as f64;

        match config.protocol {
            LLMProtocol::OpenAI | LLMProtocol::Local => {
                let client = build_openai_client(config)?
                    .completions_api()
                    .agent(&model)
                    .preamble(&system_prompt)
                    .temperature(temperature)
                    .max_tokens(RESPONSE_TOKENS as u64)
                    .build();

                let mut stream = client.stream_chat(prompt, history).await;
                Self::collect_rig_stream(&mut stream).await
            }
            LLMProtocol::Anthropic => {
                let client = build_anthropic_client(config)?
                    .agent(&model)
                    .preamble(&system_prompt)
                    .temperature(temperature)
                    .max_tokens(RESPONSE_TOKENS as u64)
                    .build();

                let mut stream = client.stream_chat(prompt, history).await;
                Self::collect_rig_stream(&mut stream).await
            }
        }
    }

    async fn stream_rig_to_sender(
        &self,
        config: &LLMProviderConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
        tx: &mpsc::Sender<StreamChunk>,
        master_mapping: std::collections::HashMap<String, String>,
    ) -> Result<(), String> {
        let (message_system_prompt, history, prompt) = Self::split_rig_chat_messages(messages);
        let system_prompt = combine_system_prompts(system_prompt, &message_system_prompt);
        let model = config.get_default_model_name();
        let temperature = config.temperature as f64;

        match config.protocol {
            LLMProtocol::OpenAI | LLMProtocol::Local => {
                let client = build_openai_client(config)?
                    .completions_api()
                    .agent(&model)
                    .preamble(&system_prompt)
                    .temperature(temperature)
                    .max_tokens(RESPONSE_TOKENS as u64)
                    .build();

                let mut stream = client.stream_chat(prompt, history).await;
                Self::send_rig_stream(&mut stream, tx, master_mapping).await
            }
            LLMProtocol::Anthropic => {
                let client = build_anthropic_client(config)?
                    .agent(&model)
                    .preamble(&system_prompt)
                    .temperature(temperature)
                    .max_tokens(RESPONSE_TOKENS as u64)
                    .build();

                let mut stream = client.stream_chat(prompt, history).await;
                Self::send_rig_stream(&mut stream, tx, master_mapping).await
            }
        }
    }

    async fn collect_rig_stream<R>(
        stream: &mut RigStreamingResult<R>,
    ) -> Result<Vec<StreamChunk>, String> {
        let mut chunks = Vec::new();
        let mut restorer = StreamingRestorer::new(std::collections::HashMap::new());

        let first = tokio::time::timeout(
            Duration::from_secs(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS),
            stream.next(),
        )
        .await
        .map_err(|_| "LLM 流式响应超时，未收到首个数据块".to_string())?;

        if let Some(item) = first {
            if Self::push_rig_stream_item(item, &mut chunks, &mut restorer)? {
                return Ok(chunks);
            }
        }

        while let Some(item) = stream.next().await {
            if Self::push_rig_stream_item(item, &mut chunks, &mut restorer)? {
                return Ok(chunks);
            }
        }

        Self::finish_rig_chunks(&mut chunks, restorer);
        Ok(chunks)
    }

    async fn send_rig_stream<R>(
        stream: &mut RigStreamingResult<R>,
        tx: &mpsc::Sender<StreamChunk>,
        master_mapping: std::collections::HashMap<String, String>,
    ) -> Result<(), String> {
        let mut restorer = StreamingRestorer::new(master_mapping);

        let first = tokio::time::timeout(
            Duration::from_secs(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS),
            stream.next(),
        )
        .await
        .map_err(|_| "LLM 流式响应超时，未收到首个数据块".to_string())?;

        if let Some(item) = first {
            if Self::send_rig_stream_item(item, tx, &mut restorer).await? {
                return Ok(());
            }
        }

        while let Some(item) = stream.next().await {
            if Self::send_rig_stream_item(item, tx, &mut restorer).await? {
                return Ok(());
            }
        }

        Self::send_done(tx, restorer).await;
        Ok(())
    }

    fn push_rig_stream_item<R>(
        item: Result<MultiTurnStreamItem<R>, rig_core::agent::StreamingError>,
        chunks: &mut Vec<StreamChunk>,
        restorer: &mut StreamingRestorer,
    ) -> Result<bool, String> {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                StreamedAssistantContent::Text(text) => {
                    let cleaned = scrub_response(&text.text);
                    let restored = restorer.feed(&cleaned);
                    if !restored.is_empty() {
                        chunks.push(StreamChunk {
                            content: restored,
                            done: false,
                            thinking: None,
                        });
                    }
                }
                StreamedAssistantContent::Reasoning(reasoning) => {
                    let thinking = reasoning.display_text();
                    if !thinking.is_empty() {
                        chunks.push(StreamChunk {
                            content: String::new(),
                            done: false,
                            thinking: Some(thinking),
                        });
                    }
                }
                StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                    if !reasoning.is_empty() {
                        chunks.push(StreamChunk {
                            content: String::new(),
                            done: false,
                            thinking: Some(reasoning),
                        });
                    }
                }
                StreamedAssistantContent::Final(_) => {}
                StreamedAssistantContent::ToolCall { .. }
                | StreamedAssistantContent::ToolCallDelta { .. } => {}
            },
            Ok(MultiTurnStreamItem::StreamUserItem(_)) => {}
            Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                Self::finish_rig_chunks(
                    chunks,
                    std::mem::replace(
                        restorer,
                        StreamingRestorer::new(std::collections::HashMap::new()),
                    ),
                );
                return Ok(true);
            }
            Ok(_) => {}
            Err(e) => return Err(format!("SSE error: {}", e)),
        }

        Ok(false)
    }

    async fn send_rig_stream_item<R>(
        item: Result<MultiTurnStreamItem<R>, rig_core::agent::StreamingError>,
        tx: &mpsc::Sender<StreamChunk>,
        restorer: &mut StreamingRestorer,
    ) -> Result<bool, String> {
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                StreamedAssistantContent::Text(text) => {
                    let cleaned = scrub_response(&text.text);
                    let restored = restorer.feed(&cleaned);
                    if !restored.is_empty() {
                        let _ = tx
                            .send(StreamChunk {
                                content: restored,
                                done: false,
                                thinking: None,
                            })
                            .await;
                    }
                }
                StreamedAssistantContent::Reasoning(reasoning) => {
                    let thinking = reasoning.display_text();
                    if !thinking.is_empty() {
                        let _ = tx
                            .send(StreamChunk {
                                content: String::new(),
                                done: false,
                                thinking: Some(thinking),
                            })
                            .await;
                    }
                }
                StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                    if !reasoning.is_empty() {
                        let _ = tx
                            .send(StreamChunk {
                                content: String::new(),
                                done: false,
                                thinking: Some(reasoning),
                            })
                            .await;
                    }
                }
                StreamedAssistantContent::Final(_) => {}
                StreamedAssistantContent::ToolCall { .. }
                | StreamedAssistantContent::ToolCallDelta { .. } => {}
            },
            Ok(MultiTurnStreamItem::StreamUserItem(_)) => {}
            Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                Self::send_done(
                    tx,
                    std::mem::replace(
                        restorer,
                        StreamingRestorer::new(std::collections::HashMap::new()),
                    ),
                )
                .await;
                return Ok(true);
            }
            Ok(_) => {}
            Err(e) => return Err(format!("SSE error: {}", e)),
        }

        Ok(false)
    }

    fn finish_rig_chunks(chunks: &mut Vec<StreamChunk>, restorer: StreamingRestorer) {
        let remaining = restorer.flush();
        if !remaining.is_empty() {
            chunks.push(StreamChunk {
                content: remaining,
                done: false,
                thinking: None,
            });
        }
        chunks.push(StreamChunk {
            content: String::new(),
            done: true,
            thinking: None,
        });
    }

    async fn send_done(tx: &mpsc::Sender<StreamChunk>, restorer: StreamingRestorer) {
        let remaining = restorer.flush();
        if !remaining.is_empty() {
            let _ = tx
                .send(StreamChunk {
                    content: remaining,
                    done: false,
                    thinking: None,
                })
                .await;
        }
        let _ = tx
            .send(StreamChunk {
                content: String::new(),
                done: true,
                thinking: None,
            })
            .await;
    }

    /// 尝试轮换指定供应商的 API Key。
    pub fn rotate_api_key(&self, provider_id: &str, failed_key_id: &str) -> Result<bool, String> {
        let mut mgr = self.providers.lock().map_err(|e| e.to_string())?;
        if let Some((next_key_id, _)) = mgr.get_next_api_key(provider_id, failed_key_id) {
            mgr.set_default_api_key(provider_id, &next_key_id)?;
            tracing::info!(
                "API Key 鏁呴殰鍒囨崲鎴愬姛锛氫緵搴斿晢 {}锛屾柊 Key ID {}",
                provider_id,
                next_key_id
            );
            Ok(true)
        } else {
            tracing::warn!(
                "API Key 鏁呴殰鍒囨崲澶辫触锛氫緵搴斿晢 {} 娌℃湁鍏朵粬鍙敤 Key",
                provider_id
            );
            Ok(false)
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
    pub fn get_config_for_provider(
        &self,
        provider_id: Option<&str>,
    ) -> Result<LLMProviderConfig, String> {
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
        // Desensitize inputs
        let mut final_system = system_prompt.to_string();
        let mut final_user = user_message.to_string();
        let mut master_mapping = std::collections::HashMap::new();
        if let Some(ref ds) = self.desensitizer {
            let user_res = ds.desensitize(user_message);
            final_user = user_res.safe_text;
            master_mapping.extend(user_res.mapping);

            let sys_res = ds.desensitize(system_prompt);
            final_system = sys_res.safe_text;
            master_mapping.extend(sys_res.mapping);
        }

        let mut attempts = 0;
        loop {
            let config = self.get_active_config()?;

            if config.get_default_key_value().is_empty() {
                return Err("LLM API key not configured".to_string());
            }

            let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
            let auth_header = format!("Bearer {}", config.get_default_key_value());
            let model = config.get_default_model_name().clone();
            let temperature = config.temperature;
            let max_tokens = config.max_tokens;

            let system_prompt_ref = &final_system;
            let user_message_ref = &final_user;
            let model_ref = &model;
            let auth_header_ref = &auth_header;
            let url_ref = &url;

            let result: Result<String, String> =
                with_retry_sync("LLM 鐢熸垚", || -> Result<String, String> {
                    let body = serde_json::json!({
                        "model": model_ref,
                        "messages": [
                            { "role": "system", "content": system_prompt_ref },
                            { "role": "user", "content": user_message_ref }
                        ],
                        "temperature": temperature,
                        "max_tokens": max_tokens,
                        "stream": false
                    });

                    let response: serde_json::Value = ureq::post(url_ref)
                        .header("Authorization", auth_header_ref)
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
                });

            if let Err(ref e) = result {
                if is_auth_error(e) && attempts < 3 {
                    let provider_id = config.id.clone();
                    let failed_key_id = config
                        .get_default_api_key()
                        .map(|k| k.id.clone())
                        .unwrap_or_default();
                    if !failed_key_id.is_empty() {
                        if let Ok(true) = self.rotate_api_key(&provider_id, &failed_key_id) {
                            attempts += 1;
                            tracing::warn!(
                                "API Key auth failed. Rotated key and retrying sync... Attempt {}",
                                attempts
                            );
                            continue;
                        }
                    }
                }
            }

            return match result {
                Ok(text) => {
                    if let Some(ref ds) = self.desensitizer {
                        Ok(ds.restore(&text, &master_mapping))
                    } else {
                        Ok(text)
                    }
                }
                Err(err) => Err(err),
            };
        }
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

    /// Perform a RAG query: hybrid search 鈫?context assembly 鈫?LLM streaming.
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

        // Step 2: Memory retrieval 鈥?search "璁板繂搴? project for relevant past memories
        if let Ok(mut memories) = hybrid_search::hybrid_search(
            query,
            Some("记忆库"),
            5, // fetch 5, apply temporal decay, then keep top 3
            embedding,
            vector_index,
            bm25,
            metadata,
        ) {
            // Apply temporal decay: older memories score lower 鈫?naturally filtered
            apply_memory_temporal_decay(&mut memories, metadata);
            for mem in memories.into_iter().take(3) {
                if !search_results.iter().any(|r| r.chunk_id == mem.chunk_id) {
                    search_results.push(mem);
                }
            }
        }

        // Step 2: Check if LLM is configured 鈥?fallback to search-only
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
        let system_tokens = token::count_tokens_with_fallback(SYSTEM_PROMPT);
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

        // Desensitize messages locally before sending to cloud
        let (desensitized_messages, master_mapping) = self.desensitize_messages(&messages);

        // Step 7: Branch by provider with Key Rotation Retry
        let mut attempts = 0;
        loop {
            let active_config = self.get_active_config()?;
            let result = self
                .rag_query_rig(&active_config, SYSTEM_PROMPT, &desensitized_messages)
                .await;

            if let Err(ref e) = result {
                if is_auth_error(e) && attempts < 3 {
                    let provider_id = active_config.id.clone();
                    let failed_key_id = active_config
                        .get_default_api_key()
                        .map(|k| k.id.clone())
                        .unwrap_or_default();
                    if !failed_key_id.is_empty() {
                        if let Ok(true) = self.rotate_api_key(&provider_id, &failed_key_id) {
                            attempts += 1;
                            tracing::warn!("API Key auth failed. Rotated key and retrying rag_query... Attempt {}", attempts);
                            continue;
                        }
                    }
                }
            }

            return match result {
                Ok(chunks) => {
                    if let Some(ref ds) = self.desensitizer {
                        let restored_chunks = chunks
                            .into_iter()
                            .map(|mut chunk| {
                                chunk.content = ds.restore(&chunk.content, &master_mapping);
                                chunk
                            })
                            .collect();
                        Ok(restored_chunks)
                    } else {
                        Ok(chunks)
                    }
                }
                Err(err) => Err(err),
            };
        }
    }

    /// Non-streaming RAG query 鈥?collects all chunks into a single response.
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
                    content_snippet: token::truncate_to_tokens(&r.content, 100),
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
                content_snippet: token::truncate_to_tokens(&r.content, 100),
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
        let (desensitized_messages, master_mapping) = self.desensitize_messages(messages);

        let mut attempts = 0;
        let mut current_config = config.clone();
        loop {
            if current_config.get_default_key_value().is_empty() {
                return Err("LLM API key not configured".to_string());
            }

            let result = self
                .chat_completion_rig(&desensitized_messages, &current_config)
                .await;

            if let Err(ref e) = result {
                if is_auth_error(e) && attempts < 3 {
                    let provider_id = current_config.id.clone();
                    let failed_key_id = current_config
                        .get_default_api_key()
                        .map(|k| k.id.clone())
                        .unwrap_or_default();
                    if !failed_key_id.is_empty() {
                        if let Ok(true) = self.rotate_api_key(&provider_id, &failed_key_id) {
                            attempts += 1;
                            if let Ok(mgr) = self.providers.lock() {
                                if let Some(updated_provider) = mgr.get_provider(&provider_id) {
                                    current_config = updated_provider.clone();
                                    tracing::warn!("API Key auth failed. Rotated key and retrying chat_completion... Attempt {}", attempts);
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            return match result {
                Ok(text) => {
                    if let Some(ref ds) = self.desensitizer {
                        Ok(ds.restore(&text, &master_mapping))
                    } else {
                        Ok(text)
                    }
                }
                Err(err) => Err(err),
            };
        }
    }

    /// Chat completion with OpenAI-style function calling support (迁移期间预留).
    /// Returns the raw content string (strips tool_calls).
    /// If `tools` is non-empty, sends with `tool_choice: "auto"`.
    #[allow(dead_code)]
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
            "model": config.get_default_model_name(),
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
                .header(
                    "Authorization",
                    format!("Bearer {}", config.get_default_key_value()),
                )
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

        tokio::time::timeout(Duration::from_secs(LLM_CALL_TIMEOUT_SECS), request_future)
            .await
            .map_err(|_| "LLM 调用超时，请检查网络连接或稍后重试".to_string())?
    }

    /// Anthropic non-streaming chat completion — POST /messages（迁移期间预留）
    ///
    /// Anthropic requires `system` as a top-level field, not in messages.
    /// Response format: `{"content":[{"type":"text","text":"..."}]}`
    #[allow(dead_code)]
    async fn chat_completion_anthropic(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        let url = if config.base_url.contains("/v1") {
            format!("{}/messages", config.base_url.trim_end_matches('/'))
        } else {
            format!("{}/v1/messages", config.base_url.trim_end_matches('/'))
        };

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
            "model": config.get_default_model_name(),
            "max_tokens": RESPONSE_TOKENS,
            "temperature": config.temperature,
            "messages": api_messages
        });

        // Anthropic: system is a top-level field, required even if empty
        if !system_prompt.is_empty() {
            body["system"] = serde_json::json!(system_prompt);
        }

        let request_future = async {
            let api_key = config.get_default_key_value();
            let response = with_anthropic_headers(self.client.post(&url), &url, &api_key)
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

        tokio::time::timeout(Duration::from_secs(LLM_CALL_TIMEOUT_SECS), request_future)
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

        // Step 1b: Memory retrieval 鈥?search "璁板繂搴? project for relevant past memories
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
        let system_tokens = token::count_tokens_with_fallback(SYSTEM_PROMPT);
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

        // Desensitize prompt messages locally before sending to cloud
        let (desensitized_messages, master_mapping) = self.desensitize_messages(&messages);

        // Step 6: Branch by provider and stream to channel with Key Rotation Retry
        let mut attempts = 0;
        loop {
            let active_config = self.get_active_config()?;
            let result = self
                .stream_rig_to_sender(
                    &active_config,
                    SYSTEM_PROMPT,
                    &desensitized_messages,
                    &tx,
                    master_mapping.clone(),
                )
                .await;

            if let Err(ref e) = result {
                if is_auth_error(e) && attempts < 3 {
                    let provider_id = active_config.id.clone();
                    let failed_key_id = active_config
                        .get_default_api_key()
                        .map(|k| k.id.clone())
                        .unwrap_or_default();
                    if !failed_key_id.is_empty() {
                        if let Ok(true) = self.rotate_api_key(&provider_id, &failed_key_id) {
                            attempts += 1;
                            tracing::warn!("API Key auth failed during streaming. Rotated key and retrying... Attempt {}", attempts);
                            continue;
                        }
                    }
                }
            }
            return result;
        }
    }

    /// Compress conversation history when it exceeds token threshold.
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
            "[Compress] Conversation tokens exceed threshold; starting summarization"
        );

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

        let head_text = head
            .iter()
            .map(|m| format!("**{}**: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let config = self.get_active_config()?;
        let summary_prompt = format!(
            "请从以下对话中提取关键上下文，保留项目背景、关键决策、待办事项和约束。\n\n---\n\n{}",
            head_text
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是一个对话摘要助手。直接输出结构化摘要，不要添加前言。".to_string(),
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
                info!(
                    target: "llm",
                    head_count = head.len(),
                    compressed_tokens = estimate_tokens(&result),
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
                    "[Compress] LLM summarization failed; keeping full history"
                );
                Ok(conversation.to_vec())
            }
        }
    }

    /// Test LLM API connectivity without requiring embedding or RAG pipeline.
    pub async fn test_connection(&self) -> Result<String, String> {
        let config = self.get_active_config()?;
        let is_local = config.protocol == LLMProtocol::Local;
        if config.get_default_key_value().is_empty() && !is_local {
            return Err("API Key 未配置".to_string());
        }

        match config.protocol {
            LLMProtocol::OpenAI | LLMProtocol::Local => {
                let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": config.get_default_model_name(),
                    "messages": [{"role": "user", "content": "Hi"}],
                    "max_tokens": 5,
                    "temperature": 0.0
                });

                let response = tokio::time::timeout(
                    Duration::from_secs(LLM_CALL_TIMEOUT_SECS),
                    self.client
                        .post(&url)
                        .header(
                            "Authorization",
                            format!("Bearer {}", config.get_default_key_value()),
                        )
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

                Ok(format!(
                    "连接成功（OpenAI / {}）",
                    config.get_default_model_name()
                ))
            }
            LLMProtocol::Anthropic => {
                let url = if config.base_url.contains("/v1") {
                    format!("{}/messages", config.base_url.trim_end_matches('/'))
                } else {
                    format!("{}/v1/messages", config.base_url.trim_end_matches('/'))
                };
                let body = serde_json::json!({
                    "model": config.get_default_model_name(),
                    "max_tokens": 5,
                    "temperature": 0.0,
                    "messages": [{"role": "user", "content": "Hi"}]
                });

                let response = tokio::time::timeout(Duration::from_secs(LLM_CALL_TIMEOUT_SECS), {
                    let api_key = config.get_default_key_value();
                    with_anthropic_headers(
                        self.client_for_config(&config)?.post(&url),
                        &url,
                        &api_key,
                    )
                    .json(&body)
                    .send()
                })
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

                Ok(format!(
                    "连接成功（Anthropic / {}）",
                    config.get_default_model_name()
                ))
            }
        }
    }

    /// Generate a fallback response when LLM is unavailable.
    pub(crate) fn fallback_response(&self, results: &[HybridSearchResult]) -> Vec<StreamChunk> {
        let answer = format!(
            "LLM 未配置（请在设置中填写 API Key），以下为知识库检索结果：\n\n{}",
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
                "**{}. {}**（来源：{} | {}）\n{}\n\n",
                i + 1,
                r.title,
                r.title,
                section,
                token::truncate_to_tokens(&r.content, 200)
            ));
        }
        output
    }
}
