//! LLM 鏈嶅姟 鈥?鏀寔 SSE 娴佸紡鐨勫鍗忚 LLM 瀹㈡埛绔?//!
//! 鏀寔 OpenAI锛圕hat Completions锛夊拰 Anthropic锛圡essages锛夊崗璁€?//! 鐢ㄦ埛鍦ㄨ缃腑閫夋嫨鎻愪緵鍟嗭紱鍚庣鐩存帴浣跨敤璇ユ彁渚涘晢鐨勫師鐢熷崗璁?鈥?鏃犻渶鍗忚杞崲銆?//!
//! 鎻愪緵瀹屾暣鐨?RAG 绠￠亾锛?//!   宓屽叆鏌ヨ 鈫?娣峰悎鎼滅储 鈫?涓婁笅鏂囩粍瑁?鈫?LLM 琛ュ叏锛圫SE锛?//!
//! 浼橀泤鍥為€€锛氬綋 LLM 涓嶅彲鐢ㄦ椂锛屼粎杩斿洖鎼滅储缁撴灉銆?
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::services::agent_timeout::{
    LLM_CALL_TIMEOUT_SECS, LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS, MAX_RETRIES, RETRY_BASE_DELAY_MS,
};
use crate::services::token;

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::hybrid_search::{self, HybridSearchResult};
use crate::services::llm_providers::{
    anthropic_messages_url, LLMProtocol, LLMProviderConfig, LLMProviderManager,
};
use crate::services::metadata::MetadataStore;
use crate::services::rig_provider::{
    build_anthropic_client, build_ollama_client, build_openai_client,
};
use crate::services::vector_index::VectorIndex;
use rig_core::agent::{MultiTurnStreamItem, StreamingResult as RigStreamingResult};
use rig_core::client::CompletionClient;
use rig_core::completion::Message as RigMessage;
use rig_core::streaming::{StreamedAssistantContent, StreamingChat};

use crate::services::verification::pipeline::VerificationPipeline;
use crate::services::verification::types::{ScenarioType, VerificationInput, VerificationReport};

// 常量

/// 系统提示词 - ERP 顾问知识助手，带有反幻觉防护。
static SYSTEM_PROMPT: &str = include_str!("../../resources/prompts/system_prompt.md");

/// 为助手响应保留的 token 数（知识密集型回答需 2048-4096 tokens）
const RESPONSE_TOKENS: u32 = 4096;

/// 非流式结构化任务的输出预算，避免推理模型只返回 thinking 不返回正文
const NON_STREAM_RESPONSE_TOKENS: u32 = 4096;

/// 推理模型只返回 thinking 时的重试输出预算
const NON_STREAM_THINKING_RETRY_TOKENS: u32 = 8192;

/// HyDE (Hypothetical Document Embeddings) 查询增强阈值：短查询（< 50 字符）时
/// 先生成假设答案再进行检索，弥合短查询与长文档之间的词汇鸿沟
/// 业界基准：nDCG@10 61.3 vs 44.5 baseline (Gao et al., 2023)
const HYDE_QUERY_MIN_CHARS: usize = 50;

/// HyDE 假设答案生成的用户提示词模板
const HYDE_PROMPT: &str = "请根据以下问题，生成一份假设的答案文档（200 字以内，仅输出答案内容，不要前言或解释）：\n";

/// 查询分类路由：根据查询特征决定处理管道
///
/// Anthropic 2025 Routing 模式：用轻量规则将查询分为三类，
/// Chitchat 跳过检索直接回复，降低延迟和无效计算。
#[derive(Debug, Clone, Copy, PartialEq)]
enum QueryCategory {
    /// 寒暄/问候 — 跳过检索，直接 LLM 回复
    Chitchat,
    /// 事实查询 — 全管道（HyDE → QueryRewrite → HybridSearch）
    Factoid,
    /// 分析型长查询 — QueryRewrite + HybridSearch，跳过 HyDE（长查询自身已足够丰富）
    Analytical,
}

/// 零延迟关键词规则分类查询
fn classify_query(query: &str) -> QueryCategory {
    let trimmed = query.trim();
    let char_count = trimmed.chars().count();

    // 寒暄模式匹配（中文 + 英文常见寒暄）
    let chitchat_patterns = [
        "你好", "您好", "嗨", "哈喽", "早上好", "下午好", "晚上好",
        "谢谢", "感谢", "多谢",
        "再见", "拜拜", "bye",
        "哈哈", "呵呵", "嗯", "哦", "好的",
        "hi", "hello", "hey", "thanks", "thank you",
    ];

    let lower = trimmed.to_lowercase();
    for pattern in &chitchat_patterns {
        if lower.starts_with(pattern) && char_count <= 10 {
            return QueryCategory::Chitchat;
        }
    }

    // 极短查询且无专业术语 → Chitchat
    if char_count < 5 {
        // 检查是否包含中文或技术性内容
        let has_chinese = trimmed.chars().any(|c| c >= '\u{4e00}' && c <= '\u{9fff}');
        let has_tech = trimmed.contains('?') || trimmed.contains('？');
        if !has_chinese && !has_tech {
            return QueryCategory::Chitchat;
        }
    }

    // 长查询（>100 字符）→ Analytical（跳过 HyDE，查询本身已足够丰富）
    if char_count > 100 {
        return QueryCategory::Analytical;
    }

    QueryCategory::Factoid
}

/// 对话压缩的 token 阈值（提升到 4000 以减少不必要压缩，保留更多对话上下文）
const COMPRESS_THRESHOLD: u32 = 4000;

/// 压缩输入的最大字符数：超过此值分批压缩，避免摘要 prompt 超出上下文窗口
const MAX_COMPRESS_INPUT_CHARS: usize = 30_000;

/// 压缩期间保持未压缩的最近消息对数
const KEEP_LAST_PAIRS: usize = 2;

/// 记忆分数时间衰减的半衰期（天）
const MEMORY_HALF_LIFE_DAYS: f64 = 30.0;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_text_extraction_skips_thinking_blocks() {
        let response = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "内部推理", "signature": "sig"},
                {"type": "text", "text": "[{\"category\":\"实施范围\"}]"}
            ]
        });

        assert!(LLMService::anthropic_response_has_thinking(&response));
        assert_eq!(
            LLMService::extract_anthropic_text(&response),
            "[{\"category\":\"实施范围\"}]"
        );
    }

    #[test]
    fn anthropic_thinking_continuation_preserves_content_blocks() {
        let original_messages = vec![serde_json::json!({
            "role": "user",
            "content": "提取合同范围"
        })];
        let response = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "内部推理", "signature": "sig"}
            ],
            "stop_reason": "max_tokens"
        });

        let messages = LLMService::build_anthropic_thinking_continuation_messages(
            &original_messages,
            &response,
        )
        .unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["signature"], "sig");
        assert_eq!(messages[2]["role"], "user");
    }
}

// 鈹€鈹€鈹€ 娴佸紡鑴辨晱杩樺師宸ュ叿 鈹€鈹€鈹€

/// 归一化占位符：去空白、转大写，用于 LLM 改写占位符时的容错匹配。
///
/// 例：`[ $_name_1 ]` → `[$_NAME_1]`。
/// StreamingRestorer 在精确匹配失败时用它做二次查找。
fn normalize_placeholder_key(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

struct StreamingRestorer {
    buffer: String,
    mapping: std::collections::HashMap<String, String>,
    /// 归一化占位符 → 原始值（去空白+大写），用于 LLM 改写占位符时的容错匹配
    normalized_mapping: std::collections::HashMap<String, String>,
}

impl StreamingRestorer {
    fn new(mapping: std::collections::HashMap<String, String>) -> Self {
        let normalized_mapping: std::collections::HashMap<String, String> = mapping
            .iter()
            .map(|(k, v)| (normalize_placeholder_key(k), v.clone()))
            .collect();
        Self {
            buffer: String::new(),
            mapping,
            normalized_mapping,
        }
    }

    fn feed(&mut self, delta: &str) -> String {
        self.buffer.push_str(delta);

        let mut output = String::new();

        loop {
            if let Some(start_idx) = self.buffer.find("[$") {
                if start_idx > 0 {
                    output.push_str(&self.buffer[..start_idx]);
                    self.buffer = self.buffer[start_idx..].to_string();
                }

                if let Some(end_idx) = self.buffer.find(']') {
                    let placeholder = &self.buffer[..=end_idx];
                    if let Some(original) = self.mapping.get(placeholder) {
                        output.push_str(original);
                    } else {
                        // 容错：LLM 可能改写占位符（加空格/改大小写），归一化后重试
                        let norm = normalize_placeholder_key(placeholder);
                        if let Some(original) = self.normalized_mapping.get(&norm) {
                            output.push_str(original);
                        } else {
                            output.push_str(placeholder);
                        }
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

// ─── 重试工具函数 ───

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
        Self {
            id: Some(uuid::Uuid::new_v4().to_string()),
            token_count: None,
        }
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
///
/// 支持可选的邻居 chunk 上下文扩展（句子窗口检索）：
/// 当提供 neighbors 时，每个检索到的 chunk 会被前后相邻 chunk 的内容包裹，
/// 形成 `<context_chunk>` 标签结构的富上下文。
pub fn assemble_context(
    results: &[HybridSearchResult],
    max_tokens: u32,
    neighbors: Option<&std::collections::HashMap<i64, (Option<String>, Option<String>)>>,
) -> String {
    let mut context = String::new();

    for result in results {
        let section = result.section_path.as_deref().unwrap_or("（无章节信息）");

        // 如果有邻居信息，构建句子窗口上下文
        if let Some(neighbor_map) = neighbors {
            if let Some((prev, next)) = neighbor_map.get(&result.chunk_id) {
                context.push_str(&format!(
                    "<context_chunk id=\"{}\" title=\"{}\" section=\"{}\">\n",
                    result.chunk_id, result.title, section
                ));
                if let Some(prev_text) = prev {
                    let truncated = token::truncate_to_tokens(prev_text, 150);
                    if !truncated.is_empty() {
                        context.push_str(&format!(
                            "  <previous_context>{}</previous_context>\n",
                            truncated
                        ));
                    }
                }
                context.push_str(&format!(
                    "  <current_chunk>{}</current_chunk>\n",
                    result.content
                ));
                if let Some(next_text) = next {
                    let truncated = token::truncate_to_tokens(next_text, 150);
                    if !truncated.is_empty() {
                        context.push_str(&format!(
                            "  <next_context>{}</next_context>\n",
                            truncated
                        ));
                    }
                }
                context.push_str("</context_chunk>\n\n");
                continue;
            }
        }

        // 无邻居信息时使用平铺格式
        let entry = format!(
            "[chunk:{} | {} | {}]\n{}\n\n",
            result.chunk_id, result.title, section, result.content
        );
        context.push_str(&entry);
    }

    // Truncate if exceeds budget
    token::truncate_to_tokens(&context, max_tokens)
}

/// Small-to-Big 检索：将子块结果映射为父块完整内容
///
/// 搜索可能命中子块（更精准的向量匹配），但上下文组装需要父块完整内容。
/// 此函数检测结果中的子块，将其替换为父块，去重后返回。
/// 回退：若子块无 parent_chunk_id（旧数据），保持不变。
fn resolve_small_to_big(
    results: Vec<HybridSearchResult>,
    metadata: &Mutex<MetadataStore>,
) -> Vec<HybridSearchResult> {
    // 分离子块和父块
    let (children, mut resolved): (Vec<_>, Vec<_>) = results
        .into_iter()
        .partition(|r| r.parent_chunk_id.is_some());

    if children.is_empty() {
        return resolved; // 无子块，直接返回
    }

    // 收集子块 ID 并查询父块
    let child_ids: Vec<i64> = children.iter().map(|r| r.chunk_id).collect();
    let parent_chunks = metadata
        .lock()
        .ok()
        .and_then(|meta| meta.get_parent_chunks_for_child_ids(&child_ids).ok())
        .unwrap_or_default();

    let parent_map: std::collections::HashMap<i64, &crate::services::metadata::ChunkMeta> = parent_chunks
        .iter()
        .map(|p| (p.id, p))
        .collect();

    // 用父块内容替换子块结果
    let mut seen_parents = std::collections::HashSet::new();
    for child in children {
        if let Some(parent_id) = child.parent_chunk_id {
            if seen_parents.insert(parent_id) {
                if let Some(parent) = parent_map.get(&parent_id) {
                    resolved.push(HybridSearchResult {
                        chunk_id: parent.id,
                        title: child.title.clone(),
                        content: parent.content.clone(),
                        score: child.score,
                        source: child.source,
                        document_id: child.document_id,
                        section_path: parent.section_path.clone(),
                        project: child.project,
                        parent_chunk_id: None, // 已解析为父块
                    });
                } else {
                    // 父块不存在（罕见：数据不一致），保留原子块
                    resolved.push(HybridSearchResult {
                        parent_chunk_id: None,
                        ..child
                    });
                }
            }
            // 已见过此父块，跳过（去重）
        }
    }

    // 按分数重新排序并限制数量
    resolved.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    resolved
}

/// 构造带上下文与问题的用户 prompt。
///
/// 当 context 为空（无检索结果 / embedding 不可用）时，回退到纯对话模式，
/// 不引用知识库内容。
///
/// 采用 Hermes 风格的 context fencing：注入的知识与记忆被包裹在 `<context>` 块中，
/// 并附系统说明，明确区分参考资料与用户真实问题。
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
        .map(|m| {
            token::count_tokens_with_fallback(&m.content)
                + token::count_tokens_with_fallback(&m.role)
        })
        .sum()
}

// 鈹€鈹€鈹€ LLM Service 鈹€鈹€鈹€

/// LLM Service 管理 API 配置并提供 RAG 查询能力。
#[derive(Clone)]
pub struct LLMService {
    /// 供应商管理器。
    providers: Arc<RwLock<LLMProviderManager>>,
    /// HTTP 客户端（可复用，连接池化）
    client: reqwest::Client,
    /// 本地数据脱敏器。
    desensitizer: Option<Arc<crate::services::desensitize::Desensitizer>>,
    /// 可选的验证管线
    pub verifier: Option<Arc<VerificationPipeline>>,
}

impl LLMService {
    /// Create a new LLM service backed by LLMProviderManager.
    pub fn new(providers: Arc<RwLock<LLMProviderManager>>) -> Self {
        Self {
            providers,
            client: reqwest::Client::new(),
            desensitizer: None,
            verifier: Some(Arc::new(VerificationPipeline::default_with_all())),
        }
    }

    /// 创建一个带脱敏器集成的 LLM 服务。
    pub fn with_desensitizer(
        providers: Arc<RwLock<LLMProviderManager>>,
        desensitizer: Arc<crate::services::desensitize::Desensitizer>,
    ) -> Self {
        Self {
            providers,
            client: reqwest::Client::new(),
            desensitizer: Some(desensitizer),
            verifier: Some(Arc::new(VerificationPipeline::default_with_all())),
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
            LLMProtocol::OpenAI => {
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
            LLMProtocol::Local => {
                let client = build_ollama_client(config)?
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

    /// 执行 RAG 查询 + 验证
    pub async fn verified_rag_query(
        &self,
        config: &LLMProviderConfig,
        system_prompt: &str,
        messages: &[ChatMessage],
        context_chunks: &[crate::services::hybrid_search::HybridSearchResult],
    ) -> Result<(Vec<StreamChunk>, Option<VerificationReport>), String> {
        // 1. 先执行原始 RAG 查询
        let chunks = self.rag_query_rig(config, system_prompt, messages).await?;

        // 2. 如果有验证器，执行验证
        let report = if let Some(ref verifier) = self.verifier {
            let full_text: String = chunks.iter().map(|c| c.content.as_str()).collect();

            let input = VerificationInput {
                generated_text: full_text,
                retrieved_chunks: context_chunks.iter().map(|c| c.content.clone()).collect(),
                chunk_titles: context_chunks.iter().map(|c| c.title.clone()).collect(),
                available_chunk_ids: context_chunks.iter().map(|c| c.chunk_id).collect(),
                query: messages
                    .last()
                    .map(|m| m.content.clone())
                    .unwrap_or_default(),
                scenario: ScenarioType::Chat,
            };

            let report = verifier.verify(&input).await;
            Some(report)
        } else {
            None
        };

        Ok((chunks, report))
    }

    /// 执行 chat_completion + 验证（适用于非 RAG 场景：文档生成、风控报告等）
    pub async fn verified_chat_completion(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
        scenario: ScenarioType,
    ) -> Result<(String, Option<VerificationReport>), String> {
        let response = self.chat_completion(messages, config).await?;

        let report = if let Some(ref verifier) = self.verifier {
            let input = VerificationInput {
                generated_text: response.clone(),
                retrieved_chunks: vec![],
                chunk_titles: vec![],
                available_chunk_ids: vec![],
                query: String::new(),
                scenario,
            };
            Some(verifier.verify(&input).await)
        } else {
            None
        };

        Ok((response, report))
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
            LLMProtocol::OpenAI => {
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
            LLMProtocol::Local => {
                let client = build_ollama_client(config)?
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
        let mut mgr = self.providers.write().map_err(|e| e.to_string())?;
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

    /// 获取默认供应商配置。
    pub fn get_active_config(&self) -> Result<LLMProviderConfig, String> {
        let mgr = self.providers.read().map_err(|e| e.to_string())?;
        mgr.get_default_runtime_provider().cloned().ok_or_else(|| {
            "未配置可用的 LLM 供应商，或所有供应商已被 Provider Policy 禁用".to_string()
        })
    }

    /// 按供应商 ID 获取配置，未指定时使用默认供应商。
    pub fn get_config_for_provider(
        &self,
        provider_id: Option<&str>,
    ) -> Result<LLMProviderConfig, String> {
        match provider_id {
            Some(id) => {
                let mgr = self.providers.read().map_err(|e| e.to_string())?;
                let provider = mgr
                    .get_provider(id)
                    .cloned()
                    .ok_or_else(|| format!("供应商 '{}' 不存在", id))?;
                mgr.assert_provider_allowed(&provider.id, None)?;
                Ok(provider)
            }
            None => self.get_active_config(),
        }
    }

    /// 按供应商和模型 ID 获取本次调用配置。
    pub fn get_config_for_provider_model(
        &self,
        provider_id: Option<&str>,
        model_id: Option<&str>,
    ) -> Result<LLMProviderConfig, String> {
        let mut config = self.get_config_for_provider(provider_id)?;
        if let Some(model_id) = model_id.map(str::trim).filter(|id| !id.is_empty()) {
            if !config.models.iter().any(|model| model.id == model_id) {
                return Err(format!(
                    "模型 '{}' 不属于供应商 '{}'",
                    model_id, config.name
                ));
            }
            {
                let mgr = self.providers.read().map_err(|e| e.to_string())?;
                mgr.assert_provider_allowed(&config.id, Some(model_id))?;
            }
            for model in &mut config.models {
                model.is_default = model.id == model_id;
            }
        } else if let Some(default_model) = config.get_default_model() {
            let mgr = self.providers.read().map_err(|e| e.to_string())?;
            mgr.assert_provider_allowed(&config.id, Some(&default_model.id))?;
        }
        Ok(config)
    }

    /// HyDE (Hypothetical Document Embeddings) 查询增强。
    ///
    /// 对于短查询（< HYDE_QUERY_MIN_CHARS 字符），调用 LLM 生成一份假设答案，
    /// 将假设答案与原始查询拼接后用于嵌入检索，弥合词汇鸿沟。
    /// 若 LLM 不可用或生成失败，回退到原始查询。
    ///
    /// 业界基准：nDCG@10 从 44.5 提升到 61.3 (Gao et al., 2023)
    pub fn enhance_query_hyde(&self, query: &str) -> String {
        let query_trimmed = query.trim();
        if query_trimmed.chars().count() >= HYDE_QUERY_MIN_CHARS {
            return query.to_string();
        }

        // 检查 LLM 是否已配置
        if !self.is_configured() {
            tracing::debug!("[HyDE] LLM 未配置，跳过查询增强");
            return query.to_string();
        }

        let user_prompt = format!("{}{}", HYDE_PROMPT, query_trimmed);

        match self.generate_text_sync(
            "你是一位 ERP 实施顾问。请生成一段简洁的知识库文档片段作为假设答案。",
            &user_prompt,
        ) {
            Ok(hypothetical) if !hypothetical.trim().is_empty() => {
                let enhanced = format!("{}\n\n---\n假设答案片段：\n{}", query, hypothetical.trim());
                tracing::debug!(
                    "[HyDE] 查询增强成功：原始 {} 字符 → 增强后 {} 字符",
                    query_trimmed.chars().count(),
                    enhanced.chars().count()
                );
                enhanced
            }
            Err(e) => {
                tracing::warn!("[HyDE] 假设答案生成失败: {}，回退到原始查询", e);
                query.to_string()
            }
            Ok(_) => {
                tracing::debug!("[HyDE] 假设答案为空，回退到原始查询");
                query.to_string()
            }
        }
    }

    /// Chitchat 快速回复：跳过检索管道，直接 LLM 回复寒暄语
    ///
    /// 零延迟路由，避免对"你好""谢谢"等寒暄触发嵌入计算和混合搜索。
    async fn chitchat_reply(
        &self,
        query: &str,
        conversation_history: &[ChatMessage],
    ) -> Result<Vec<StreamChunk>, String> {
        if !self.is_configured() {
            return Ok(vec![StreamChunk {
                content: "您好！我是金蝶ERP实施顾问助手，请问有什么可以帮您的？".to_string(),
                done: true,
                thinking: None,
            }]);
        }

        let config = self.get_active_config()?;
        let mut messages: Vec<ChatMessage> = Vec::new();
        for msg in conversation_history.iter().rev().take(4).rev() {
            messages.push(msg.clone());
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: query.to_string(),
        });

        // 寒暄回复（"你好""谢谢"）由 LLM 独立生成，不引用用户消息中的具体内容，
        // 因此无需用 master_mapping 还原脱敏占位符。脱敏仍执行以保证发往 LLM 的内容不含敏感信息。
        let (desensitized_messages, _master_mapping) = self.desensitize_messages(&messages);
        self.rag_query_rig(&config, SYSTEM_PROMPT, &desensitized_messages)
            .await
    }

    /// 查询重写（Query Rewriting）：将多轮对话中的模糊引用改写为独立查询。
    ///
    /// 当对话历史非空时，用 LLM 将当前查询 + 最近 2 轮对话重写为上下文完整的独立查询。
    /// 若 LLM 不可用或对话历史为空，回退到原始查询。
    pub fn rewrite_query(&self, query: &str, conversation_history: &[ChatMessage]) -> String {
        if conversation_history.is_empty() {
            return query.to_string();
        }

        if !self.is_configured() {
            return query.to_string();
        }

        // 只取最近 2 轮对话（4 条消息）作为上下文
        let recent: Vec<&ChatMessage> = conversation_history
            .iter()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let history_text = recent
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let rewrite_prompt = format!(
            "请将以下对话中的最后一条用户问题改写为独立、完整的查询语句，包含足够的上下文信息（如项目名、模块名等）。只输出改写后的查询，不要添加任何解释。\n\n对话历史：\n{}\n\n当前问题：{}\n\n改写后的查询：",
            history_text, query
        );

        match self.generate_text_sync(
            "你是一个查询改写助手。将依赖上下文的模糊问题改写为独立查询。",
            &rewrite_prompt,
        ) {
            Ok(rewritten) if !rewritten.trim().is_empty() => {
                let rewritten = rewritten.trim().to_string();
                if rewritten != query.trim() {
                    tracing::debug!(
                        "[QueryRewrite] \"{}\" → \"{}\"",
                        query,
                        rewritten
                    );
                }
                rewritten
            }
            _ => query.to_string(),
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

    /// 检查 LLM 是否已配置（已设 API key，或使用本地模型）。
    pub fn is_configured(&self) -> bool {
        self.get_active_config()
            .map(|cfg| cfg.is_configured())
            .unwrap_or(false)
    }

    /// 执行 RAG 查询：混合检索 → 上下文组装 → LLM 流式输出。
    ///
    /// 返回 `StreamChunk` 的异步流。若 LLM 不可用，回退到单 chunk 的纯检索结果。
    ///
    /// 按供应商分支：OpenAI 使用 /chat/completions，Anthropic 使用 /messages。
    pub async fn rag_query(
        &self,
        query: &str,
        project_id: Option<&str>,
        conversation_history: Vec<ChatMessage>,
        embedding: &RwLock<EmbeddingService>,
        vector_index: &RwLock<VectorIndex>,
        bm25: &RwLock<BM25Service>,
        metadata: &Mutex<MetadataStore>,
    ) -> Result<Vec<StreamChunk>, String> {
        // Step 0: 查询分类路由 — Chitchat 跳过检索直接回复
        let query_category = classify_query(query);
        if query_category == QueryCategory::Chitchat {
            return self.chitchat_reply(query, &conversation_history).await;
        }

        // Step 0a: Query Rewriting — 多轮对话中模糊引用改写为独立查询
        let standalone_query = self.rewrite_query(query, &conversation_history);
        // Step 0b: HyDE 查询增强 — 短查询生成假设答案弥合词汇鸿沟
        //          Analytical（>100 字符）查询跳过 HyDE，查询本身已足够丰富
        let enhanced_query = if query_category == QueryCategory::Analytical {
            standalone_query.clone()
        } else {
            self.enhance_query_hyde(&standalone_query)
        };

        // Step 1: Hybrid search (KB documents)
        let mut search_results = hybrid_search::hybrid_search(
            &enhanced_query,
            project_id,
            &[],
            15,
            embedding,
            vector_index,
            bm25,
            metadata,
            None,
            None,
        )?;

        // Step 2: Memory retrieval — search "记忆库" project for relevant past memories
        if let Ok(mut memories) = hybrid_search::hybrid_search(
            &enhanced_query,
            Some("记忆库"),
            &[],
            5,
            embedding,
            vector_index,
            bm25,
            metadata,
            None,
            None,
        ) {
            // Apply temporal decay: older memories score lower 鈫?naturally filtered
            apply_memory_temporal_decay(&mut memories, metadata);
            for mem in memories.into_iter().take(3) {
                if !search_results.iter().any(|r| r.chunk_id == mem.chunk_id) {
                    search_results.push(mem);
                }
            }
        }

        // Small-to-Big: resolve child chunks to parent chunks for richer context
        search_results = resolve_small_to_big(search_results, metadata);

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

        // Step 5: Assemble context with sentence window (neighbor chunks)
        let system_tokens = token::count_tokens_with_fallback(SYSTEM_PROMPT);
        let budget = config
            .max_tokens
            .saturating_sub(system_tokens + RESPONSE_TOKENS + 200);
        // 获取邻居 chunk 用于句子窗口上下文扩展
        let chunk_ids: Vec<i64> = search_results.iter().map(|r| r.chunk_id).collect();
        let neighbors = metadata
            .lock()
            .ok()
            .and_then(|meta| meta.get_chunk_neighbors_batch(&chunk_ids).ok());
        let context = assemble_context(&search_results, budget, neighbors.as_ref());
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

    /// 非流式 RAG 查询 —— 收集所有 chunk 为单条响应。
    pub async fn rag_query_sync(
        &self,
        query: &str,
        project_id: Option<&str>,
        conversation_history: Vec<ChatMessage>,
        embedding: &RwLock<EmbeddingService>,
        vector_index: &RwLock<VectorIndex>,
        bm25: &RwLock<BM25Service>,
        metadata: &Mutex<MetadataStore>,
    ) -> Result<RAGResponse, String> {
        // Step 0: 查询分类路由 — Chitchat 跳过检索直接回复
        if classify_query(query) == QueryCategory::Chitchat {
            if !self.is_configured() {
                return Ok(RAGResponse {
                    answer: "您好！我是金蝶ERP实施顾问助手，请问有什么可以帮您的？".to_string(),
                    sources: Vec::new(),
                    llm_available: false,
                });
            }
            let chunks = self.chitchat_reply(query, &conversation_history).await?;
            let answer: String = chunks.iter().map(|c| c.content.as_str()).collect();
            return Ok(RAGResponse {
                answer,
                sources: Vec::new(),
                llm_available: true,
            });
        }

        // Step 0a: Query Rewriting — 多轮对话中模糊引用改写为独立查询
        let standalone_query = self.rewrite_query(query, &conversation_history);
        // Step 0b: HyDE 查询增强 — 短查询生成假设答案弥合词汇鸿沟
        //          Analytical（>100 字符）查询跳过 HyDE
        let query_category = classify_query(query);
        let enhanced_query = if query_category == QueryCategory::Analytical {
            standalone_query.clone()
        } else {
            self.enhance_query_hyde(&standalone_query)
        };

        let search_results = hybrid_search::hybrid_search(
            &enhanced_query,
            project_id,
            &[],
            15,
            embedding,
            vector_index,
            bm25,
            metadata,
            None,
            None,
        )?;

        // Small-to-Big: resolve child chunks to parent chunks
        let search_results = resolve_small_to_big(search_results, metadata);

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

    /// 简单聊天补全（非流式，不走 RAG 上下文）。
    ///
    /// 直接将消息发送至 LLM API 并返回响应文本。
    /// 用于字段生成等非 RAG 任务。
    ///
    /// 按供应商分支：OpenAI 使用 /chat/completions，Anthropic 使用 /messages。
    pub async fn chat_completion(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        self.chat_completion_internal(messages, config, true).await
    }

    /// 非流式文本生成，不执行脱敏占位替换。
    pub async fn chat_completion_unmasked(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        self.chat_completion_internal(messages, config, false).await
    }

    async fn chat_completion_internal(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
        enable_desensitize: bool,
    ) -> Result<String, String> {
        let (request_messages, master_mapping) = if enable_desensitize {
            self.desensitize_messages(messages)
        } else {
            (messages.to_vec(), std::collections::HashMap::new())
        };

        let mut attempts = 0;
        let mut current_config = config.clone();
        loop {
            if current_config.get_default_key_value().is_empty()
                && current_config.protocol != LLMProtocol::Local
            {
                return Err("LLM API key not configured".to_string());
            }

            let result = self
                .chat_completion_native(&request_messages, &current_config)
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
                            if let Ok(mgr) = self.providers.read() {
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
                    if enable_desensitize {
                        if let Some(ref ds) = self.desensitizer {
                            Ok(ds.restore(&text, &master_mapping))
                        } else {
                            Ok(text)
                        }
                    } else {
                        Ok(text)
                    }
                }
                Err(err) => Err(err),
            };
        }
    }

    async fn chat_completion_native(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        match config.protocol {
            LLMProtocol::OpenAI => {
                self.chat_completion_openai_with_tools(messages, config, &[], false)
                    .await
            }
            LLMProtocol::Anthropic => self.chat_completion_anthropic(messages, config).await,
            LLMProtocol::Local => self.chat_completion_local(messages, config).await,
        }
    }

    fn json_response_preview(json: &serde_json::Value) -> String {
        let text = json.to_string();
        let preview: String = text.chars().take(800).collect();
        if text.chars().count() > 800 {
            format!("{}...", preview)
        } else {
            preview
        }
    }
    /// OpenAI 兼容非流式文本生成，保留工具调用参数能力。
    /// 返回原始内容字符串，不返回工具调用。
    /// 当 tools 非空时，使用自动工具选择。
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
            "max_tokens": NON_STREAM_RESPONSE_TOKENS,
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
                return Err(format!(
                    "OpenAI 兼容端点返回空内容，原始响应预览: {}",
                    Self::json_response_preview(&json)
                ));
            }

            Ok(content)
        };

        tokio::time::timeout(Duration::from_secs(LLM_CALL_TIMEOUT_SECS), request_future)
            .await
            .map_err(|_| "LLM 调用超时，请检查网络连接或稍后重试".to_string())?
    }

    /// Anthropic 非流式文本生成，使用 /messages。
    async fn chat_completion_anthropic(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        let url = anthropic_messages_url(&config.base_url);

        // 提取 system 消息，Anthropic 要求放在顶层字段。
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
            "max_tokens": NON_STREAM_RESPONSE_TOKENS,
            "temperature": config.temperature,
            "messages": api_messages.clone()
        });

        if !system_prompt.is_empty() {
            body["system"] = serde_json::json!(system_prompt);
        }

        let request_future = async {
            let api_key = config.get_default_key_value();
            let mut last_thinking_response: Option<serde_json::Value> = None;
            let mut continued_from_thinking = false;

            for _attempt in 0..=1 {
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

                let content = Self::extract_anthropic_text(&json);
                if !content.trim().is_empty() {
                    return Ok(content);
                }

                if Self::anthropic_response_has_thinking(&json) {
                    last_thinking_response = Some(json.clone());
                    if !continued_from_thinking {
                        body["max_tokens"] = serde_json::json!(NON_STREAM_THINKING_RETRY_TOKENS);
                        body["messages"] = serde_json::json!(
                            Self::build_anthropic_thinking_continuation_messages(
                                &api_messages,
                                &json
                            )?
                        );
                        let extra =
                            "请基于上一条 assistant 的 thinking 继续完成最终回答，在 text 中输出用户要求的结果。";
                        let next_system = body
                            .get("system")
                            .and_then(|value| value.as_str())
                            .map(|system| format!("{}\n\n{}", system, extra))
                            .unwrap_or_else(|| extra.to_string());
                        body["system"] = serde_json::json!(next_system);
                        continued_from_thinking = true;
                        continue;
                    }
                }

                return Err(format!(
                    "Anthropic 兼容端点返回空内容，可能是协议选择或响应格式不匹配。原始响应预览: {}",
                    Self::json_response_preview(&json)
                ));
            }

            if let Some(json) = last_thinking_response {
                let stop_reason = json["stop_reason"].as_str().unwrap_or("未知");
                return Err(format!(
                    "Anthropic 兼容端点返回 thinking 后仍未返回最终 text。stop_reason={}。请检查模型输出预算、reasoning 配置或端点响应格式。原始响应预览: {}",
                    stop_reason,
                    Self::json_response_preview(&json)
                ));
            }

            Err("Anthropic 兼容端点返回空内容".to_string())
        };

        tokio::time::timeout(Duration::from_secs(LLM_CALL_TIMEOUT_SECS), request_future)
            .await
            .map_err(|_| "LLM 调用超时，请检查网络连接或稍后重试".to_string())?
    }

    fn extract_anthropic_text(json: &serde_json::Value) -> String {
        json["content"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    }

    fn anthropic_response_has_thinking(json: &serde_json::Value) -> bool {
        json["content"]
            .as_array()
            .map(|arr| {
                arr.iter().any(|block| {
                    block.get("thinking").is_some()
                        || block.get("type").and_then(|value| value.as_str()) == Some("thinking")
                })
            })
            .unwrap_or(false)
    }

    fn build_anthropic_thinking_continuation_messages(
        original_messages: &[serde_json::Value],
        response_json: &serde_json::Value,
    ) -> Result<Vec<serde_json::Value>, String> {
        let content = response_json
            .get("content")
            .and_then(|value| value.as_array())
            .filter(|items| !items.is_empty())
            .ok_or_else(|| "Anthropic thinking 续写失败：响应缺少 content 数组".to_string())?;

        let mut messages = original_messages.to_vec();
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": content
        }));
        messages.push(serde_json::json!({
            "role": "user",
            "content": "请继续完成刚才的回答，直接输出最终 text 内容。若任务要求 JSON，只输出 JSON，不要重复思考过程。"
        }));
        Ok(messages)
    }

    /// 本地模型非流式文本生成，使用 Ollama /api/chat。
    async fn chat_completion_local(
        &self,
        messages: &[ChatMessage],
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        let url = format!("{}/api/chat", config.base_url.trim_end_matches('/'));
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
            "model": config.get_default_model_name(),
            "messages": api_messages,
            "options": {
                "num_predict": NON_STREAM_RESPONSE_TOKENS
            },
            "stream": false
        });

        let request_future = async {
            let response = self
                .client_for_config(config)?
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("本地模型请求失败: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let body_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<unreadable>".to_string());
                return Err(format!("本地模型 API 返回错误 ({})：{}", status, body_text));
            }

            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| format!("解析本地模型响应失败: {}", e))?;

            let content = json["message"]["content"]
                .as_str()
                .or_else(|| json["response"].as_str())
                .unwrap_or("")
                .to_string();

            if content.trim().is_empty() {
                return Err(format!(
                    "本地模型返回空内容，原始响应预览: {}",
                    Self::json_response_preview(&json)
                ));
            }

            Ok(content)
        };

        tokio::time::timeout(Duration::from_secs(LLM_CALL_TIMEOUT_SECS), request_future)
            .await
            .map_err(|_| "本地模型调用超时，请检查模型服务是否正常".to_string())?
    }

    /// 基于 channel 的 RAG 流式查询。
    ///
    /// 与 `rag_query()` 行为一致，但每收到一个 `StreamChunk` 即通过 channel 推送，
    /// 实现前端实时流式渲染。调用方负责消费完所有 chunk。
    ///
    /// 若提供 `precomputed_results`，跳过混合检索步骤
    /// （适用于调用方已先做过检索以提取来源信息的场景）。
    pub async fn rag_query_to_sender(
        &self,
        query: &str,
        project_id: Option<&str>,
        conversation_history: Vec<ChatMessage>,
        embedding: &RwLock<EmbeddingService>,
        vector_index: &RwLock<VectorIndex>,
        bm25: &RwLock<BM25Service>,
        metadata: &Mutex<MetadataStore>,
        tx: mpsc::Sender<StreamChunk>,
        precomputed_results: Option<Vec<HybridSearchResult>>,
    ) -> Result<(), String> {
        // Step 0: 查询分类路由 — Chitchat 跳过检索直接回复
        let query_category = classify_query(query);
        if query_category == QueryCategory::Chitchat {
            if !self.is_configured() {
                let _ = tx.send(StreamChunk { content: "您好！我是金蝶ERP实施顾问助手，请问有什么可以帮您的？".to_string(), done: true, thinking: None }).await;
                return Ok(());
            }
            let chunks = self.chitchat_reply(query, &conversation_history).await?;
            for chunk in chunks {
                let _ = tx.send(chunk).await;
            }
            return Ok(());
        }

        // Step 0a: Query Rewriting — 多轮对话中模糊引用改写为独立查询
        let standalone_query = self.rewrite_query(query, &conversation_history);
        // Step 0b: HyDE 查询增强
        //          Analytical（>100 字符）查询跳过 HyDE
        let enhanced_query = if query_category == QueryCategory::Analytical {
            standalone_query.clone()
        } else {
            self.enhance_query_hyde(&standalone_query)
        };

        // Step 1: Hybrid search (skip if precomputed)
        let mut search_results: Vec<HybridSearchResult> = match precomputed_results {
            Some(results) => results,
            None => hybrid_search::hybrid_search(
                &enhanced_query,
                project_id,
                &[],
                15,
                embedding,
                vector_index,
                bm25,
                metadata,
                None,
                None,
            )?,
        };

        // Step 1b: Memory retrieval — search "记忆库" project for relevant past memories
        if let Ok(mut memories) = hybrid_search::hybrid_search(
            &enhanced_query,
            Some("记忆库"),
            &[],
            5,
            embedding,
            vector_index,
            bm25,
            metadata,
            None,
            None,
        ) {
            apply_memory_temporal_decay(&mut memories, metadata);
            for mem in memories.into_iter().take(3) {
                if !search_results.iter().any(|r| r.chunk_id == mem.chunk_id) {
                    search_results.push(mem);
                }
            }
        }

        // Small-to-Big: resolve child chunks to parent chunks for richer context
        search_results = resolve_small_to_big(search_results, metadata);

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

        // Step 4: Assemble context with sentence window (neighbor chunks)
        let system_tokens = token::count_tokens_with_fallback(SYSTEM_PROMPT);
        let budget = config
            .max_tokens
            .saturating_sub(system_tokens + RESPONSE_TOKENS + 200);
        // 获取邻居 chunk 用于句子窗口上下文扩展
        let chunk_ids: Vec<i64> = search_results.iter().map(|r| r.chunk_id).collect();
        let neighbors = metadata
            .lock()
            .ok()
            .and_then(|meta| meta.get_chunk_neighbors_batch(&chunk_ids).ok());
        let context = assemble_context(&search_results, budget, neighbors.as_ref());
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

    /// 当对话历史超过 token 阈值时压缩。
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

        // 分批压缩：若 head_text 过长，分批生成摘要再合并
        let summary = if head_text.chars().count() > MAX_COMPRESS_INPUT_CHARS {
            self.compress_in_batches(&head_text, &config).await?
        } else {
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

            self.chat_completion(&messages, &config).await?
        };

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

    /// 分批压缩超长对话历史：将 head_text 分批次压缩，合并后再次压缩为最终摘要
    async fn compress_in_batches(
        &self,
        head_text: &str,
        config: &LLMProviderConfig,
    ) -> Result<String, String> {
        let chars: Vec<char> = head_text.chars().collect();
        let batch_size = MAX_COMPRESS_INPUT_CHARS / 2; // 每批 15K 字符
        let mut batch_summaries: Vec<String> = Vec::new();
        let mut start = 0usize;

        while start < chars.len() {
            let end = (start + batch_size).min(chars.len());
            let batch: String = chars[start..end].iter().collect();

            let batch_prompt = format!(
                "请从以下对话片段（第 {}/{} 部分）中提取关键上下文，保留项目背景、关键决策、待办事项和约束。\n\n---\n\n{}",
                batch_summaries.len() + 1,
                ((chars.len() + batch_size - 1) / batch_size),
                batch
            );

            let messages = vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: "你是一个对话摘要助手。直接输出结构化摘要，不要添加前言。".to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: batch_prompt,
                },
            ];

            match self.chat_completion(&messages, config).await {
                Ok(summary) if !summary.trim().is_empty() => {
                    batch_summaries.push(summary.trim().to_string());
                }
                Err(e) => {
                    tracing::warn!("[Compress] 批次 {} 压缩失败: {}", batch_summaries.len() + 1, e);
                }
                _ => {}
            }

            if end >= chars.len() {
                break;
            }
            start = end;
        }

        if batch_summaries.is_empty() {
            return Err("所有批次压缩均失败".to_string());
        }

        if batch_summaries.len() == 1 {
            return Ok(batch_summaries.into_iter().next().unwrap());
        }

        // 合并多个批次摘要为最终摘要
        let combined = batch_summaries.join("\n\n---\n\n");
        let merge_prompt = format!(
            "以下是对话历史的多段摘要。请将它们合并为一份连贯的摘要，保留项目背景、关键决策、待办事项和约束。\n\n---\n\n{}",
            combined
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是一个对话摘要助手。直接输出结构化摘要，不要添加前言。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: merge_prompt,
            },
        ];

        self.chat_completion(&messages, config).await
    }

    /// 测试 LLM API 连通性，无需 embedding 或 RAG 管线。
    pub async fn test_connection(&self) -> Result<String, String> {
        let config = self.get_active_config()?;
        let is_local = config.protocol == LLMProtocol::Local;
        if config.get_default_key_value().is_empty() && !is_local {
            return Err("API Key 未配置".to_string());
        }

        match config.protocol {
            LLMProtocol::OpenAI => {
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
            LLMProtocol::Local => {
                let url = format!("{}/api/chat", config.base_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "model": config.get_default_model_name(),
                    "messages": [{"role": "user", "content": "Hi"}],
                    "stream": false
                });

                let response = tokio::time::timeout(
                    Duration::from_secs(LLM_CALL_TIMEOUT_SECS),
                    self.client_for_config(&config)?
                        .post(&url)
                        .json(&body)
                        .send(),
                )
                .await
                .map_err(|_| "Ollama 连接测试超时".to_string())?
                .map_err(|e| format!("连接失败：{}", e))?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "<unreadable>".to_string());
                    return Err(format!("Ollama API 返回错误 ({})：{}", status, body_text));
                }

                Ok(format!(
                    "连接成功（Ollama / {}）",
                    config.get_default_model_name()
                ))
            }
            LLMProtocol::Anthropic => {
                let url = anthropic_messages_url(&config.base_url);
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

    /// 在 LLM 不可用时生成兜底响应。
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

    /// 将检索结果格式化为可读的纯文本答案。
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
