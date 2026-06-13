//! ReAct 事件定义
//!
//! ReActEvent 枚举用于前端 SSE 事件格式，
//! 被 rig_agent、question_tool 等模块使用。

use serde::{Deserialize, Serialize};

use crate::services::question_tool::ClarificationPayload;
use crate::services::verification::types::VerificationReport;

/// ReAct 事件 — 通过 SSE 流式发送给前端
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReActEvent {
    #[serde(rename = "thinking")]
    Thinking { session_id: String, content: String },
    #[serde(rename = "tool_call")]
    ToolCall {
        session_id: String,
        name: String,
        args: String,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        session_id: String,
        name: String,
        result: String,
    },
    #[serde(rename = "text_delta")]
    TextDelta { session_id: String, content: String },
    #[serde(rename = "error")]
    Error {
        session_id: String,
        message: String,
        /// 机器可读错误码。
        ///
        /// - `LLM_INVALID_KEY` — LLM API Key 失效/过期/被吊销（HTTP 401）。
        ///   前端识别后弹"配置 API Key"对话框。
        /// - 其他 — 普通错误，前端走默认 toast 提示。
        ///
        /// 字段缺失时（向前兼容）视为 None，前端按"普通错误"处理。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
        /// 当 error_code = `LLM_INVALID_KEY` 时携带，指向具体供应商 id。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
    },
    #[serde(rename = "done")]
    Done {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        verification_report: Option<VerificationReport>,
    },
    /// Agent uses the `question` tool to ask the user a clarification question.
    /// `payload` contains question_id, prompt, mode, and options.
    #[serde(rename = "clarification")]
    Clarification {
        session_id: String,
        payload: ClarificationPayload,
    },
    #[serde(rename = "plan_generated")]
    PlanGenerated {
        session_id: String,
        steps: Vec<crate::services::planner::PlanStep>,
    },
    #[serde(rename = "step_start")]
    StepStart {
        session_id: String,
        step_index: usize,
        total_steps: usize,
        description: String,
    },
    #[serde(rename = "step_result")]
    StepResult {
        session_id: String,
        step_index: usize,
        result: String,
        success: bool,
    },
    #[serde(rename = "replan")]
    Replan { session_id: String, reason: String },
    #[serde(rename = "planner_timeout")]
    PlannerTimeout { session_id: String, message: String },
}

impl ReActEvent {
    /// 构造普通 Error 事件（不带 error_code，前端走默认 toast 提示）
    pub fn error(session_id: impl Into<String>, message: impl Into<String>) -> Self {
        ReActEvent::Error {
            session_id: session_id.into(),
            message: message.into(),
            error_code: None,
            provider_id: None,
        }
    }

    /// 构造 LLM 错误事件，自动检测 401 模式并附带 error_code
    ///
    /// P0-5 修复：LLM 调用失败时不再只是"流式错误: ..."字符串，
    ///           而是把 401 / Invalid API Key 升级为结构化错误，
    ///           前端识别后弹"配置 API Key"对话框。
    pub fn llm_error(
        session_id: impl Into<String>,
        message: impl Into<String>,
        provider_id: impl Into<String>,
    ) -> Self {
        let msg = message.into();
        let is_auth = is_llm_auth_error(&msg);
        ReActEvent::Error {
            session_id: session_id.into(),
            message: msg,
            error_code: if is_auth {
                Some("LLM_INVALID_KEY".to_string())
            } else {
                None
            },
            provider_id: Some(provider_id.into()),
        }
    }
}

/// 复用 `AppError::classify_llm_error` 的 401 检测规则，
/// 但直接返回 bool，避免 LLM 调用点要构造一个完整 AppError 又丢弃。
///
/// 单一来源原则：检测字符串与 `error.rs` 保持一致；
/// 若未来扩展新关键字，只改这里和 `error.rs` 即可。
pub(crate) fn is_llm_auth_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("invalid api key")
        || lower.contains("incorrect api key")
        || lower.contains("authentication")
        || lower.contains("api key not configured")
}

// ── 测试 ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_llm_auth_error_matches_common_401_messages() {
        for s in &[
            "OpenAI API error (401): Incorrect API key provided",
            "Anthropic API error (401 Unauthorized)",
            "Authentication failed: API key not configured",
            "401 unauthorized",
        ] {
            assert!(is_llm_auth_error(s), "应被识别为 auth error: {}", s);
        }
    }

    #[test]
    fn is_llm_auth_error_ignores_other_errors() {
        for s in &["LLM 调用超时", "HTTP 500", "网络错误"] {
            assert!(!is_llm_auth_error(s), "不应被识别为 auth error: {}", s);
        }
    }

    #[test]
    fn llm_error_event_carries_code_for_401() {
        let evt = ReActEvent::llm_error("sid-1", "401 Unauthorized", "openai");
        match evt {
            ReActEvent::Error {
                error_code,
                provider_id,
                ..
            } => {
                assert_eq!(error_code.as_deref(), Some("LLM_INVALID_KEY"));
                assert_eq!(provider_id.as_deref(), Some("openai"));
            }
            _ => panic!("expected Error variant"),
        }
    }

    #[test]
    fn llm_error_event_omits_code_for_other_errors() {
        let evt = ReActEvent::llm_error("sid-1", "LLM 调用超时", "openai");
        match evt {
            ReActEvent::Error {
                error_code,
                provider_id,
                ..
            } => {
                assert!(error_code.is_none());
                assert_eq!(provider_id.as_deref(), Some("openai"));
            }
            _ => panic!("expected Error variant"),
        }
    }
}
