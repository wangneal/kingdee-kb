//! 全局错误类型定义。所有公开内部函数返回 [`AppResult<T>`]。
//!
//! 迁移路径：旧 `Result<_, String>` → 新 `AppResult<T>`（用 `?` + `anyhow::Context` 加上下文）。

use std::path::PathBuf;

use serde::{Serialize, Serializer};
use thiserror::Error;

/// 应用统一错误类型。
#[derive(Error, Debug)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO 错误 ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),

    #[error("配置错误: {0}")]
    Config(String),

    #[error("API 错误: {0}")]
    Api(String),

    /// 401 / Key 失效。区别于 `Api` 是因为前端要弹"配置 Key"对话框
    /// 而非"重试"按钮。
    #[error("LLM API Key 无效（供应商: {provider_id}）")]
    LlmInvalidKey {
        provider_id: String,
        /// 原始服务端响应（已截断到 120 字），帮助用户诊断 Key 失效原因
        detail: String,
    },

    #[error("参数错误: {0}")]
    InvalidArgument(String),

    #[error("未找到: {0}")]
    NotFound(String),

    #[error("内部错误: {0}")]
    Internal(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl AppError {
    /// 机器可读错误码（前端 switch 用）
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Database(_) => "DATABASE",
            AppError::Io { .. } => "IO",
            AppError::Json(_) => "JSON",
            AppError::Config(_) => "CONFIG",
            AppError::Api(_) => "API",
            AppError::LlmInvalidKey { .. } => "LLM_INVALID_KEY",
            AppError::InvalidArgument(_) => "INVALID_ARGUMENT",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::Internal(_) => "INTERNAL",
            AppError::Other(_) => "OTHER",
        }
    }

    /// 用户可读的错误消息（去技术细节）
    pub fn user_message(&self) -> String {
        match self {
            AppError::Database(_) => "数据库操作失败".to_string(),
            AppError::Io { path, .. } => format!("文件操作失败: {}", path.display()),
            AppError::Json(_) => "数据格式错误".to_string(),
            AppError::Config(msg) => format!("配置错误: {}", msg),
            AppError::Api(msg) => format!("外部服务调用失败: {}", msg),
            AppError::LlmInvalidKey { provider_id, detail } => {
                format!(
                    "LLM 供应商「{}」的 API Key 已失效（{}）。请到设置页更换 Key 后重试。",
                    provider_id,
                    truncate_for_user(detail, 120)
                )
            }
            AppError::InvalidArgument(msg) => msg.clone(),
            AppError::NotFound(msg) => format!("未找到: {}", msg),
            AppError::Internal(msg) => format!("内部错误: {}", msg),
            AppError::Other(e) => e.to_string(),
        }
    }

    /// 包装 IO 错误并附加路径上下文
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        AppError::Io {
            path: path.into(),
            source,
        }
    }

    /// 401 / unauthorized / invalid api key → LlmInvalidKey；其他 → Api 透传
    pub fn classify_llm_error(provider_id: impl Into<String>, err: &str) -> Self {
        let lower = err.to_ascii_lowercase();
        let is_auth = lower.contains("401")
            || lower.contains("unauthorized")
            || lower.contains("invalid api key")
            || lower.contains("incorrect api key")
            || lower.contains("authentication")
            || lower.contains("api key not configured");
        if is_auth {
            AppError::LlmInvalidKey {
                provider_id: provider_id.into(),
                detail: err.to_string(),
            }
        } else {
            AppError::Api(err.to_string())
        }
    }
}

/// 应用统一 Result 别名
pub type AppResult<T> = Result<T, AppError>;

// Tauri IPC 序列化：前端拿到 {code, message, provider_id?}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 3)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.user_message())?;
        // provider_id 仅对 LlmInvalidKey 有意义，前端会忽略非该变体的字段
        if let AppError::LlmInvalidKey { provider_id, .. } = self {
            s.serialize_field("provider_id", provider_id)?;
        }
        s.end()
    }
}

/// 把超长 detail 截断到 max_chars，超长部分用 "…" 收尾。
/// Unicode 字符按 char 计数（不是字节），避免中文截半个字符。
fn truncate_for_user(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars().filter(|c| !c.is_control());
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

// 测试

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_is_stable() {
        // 前端会 switch case 匹配这些字符串，**禁止随意改名**
        assert_eq!(
            AppError::Database(rusqlite::Error::QueryReturnedNoRows).code(),
            "DATABASE"
        );
        assert_eq!(AppError::Config("foo".into()).code(), "CONFIG");
        assert_eq!(AppError::NotFound("bar".into()).code(), "NOT_FOUND");
    }

    #[test]
    fn user_message_does_not_leak_io_kind() {
        // 内部 io::ErrorKind 不应该出现在 user_message 里
        let err = AppError::io("/secret/path/foo.db", std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        ));
        let msg = err.user_message();
        assert!(msg.contains("foo.db"));
        // 给终端用户看，隐藏底层技术细节
        assert!(!msg.contains("permission denied"));
    }

    #[test]
    fn serialize_emits_code_and_message() {
        let err = AppError::NotFound("widget".into());
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":\"NOT_FOUND\""));
        assert!(json.contains("widget"));
    }

    #[test]
    fn from_io_error_via_path_helper() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        let err: AppError = AppError::io("/tmp/missing", io);
        assert!(matches!(err, AppError::Io { .. }));
    }

    /// 回归：LlmInvalidKey 必须能被前端 switch 识别，序列化要带 provider_id
    #[test]
    fn llm_invalid_key_is_structured_and_serializable() {
        let err = AppError::LlmInvalidKey {
            provider_id: "openai".to_string(),
            detail: "Incorrect API key provided: sk-***".to_string(),
        };

        assert_eq!(err.code(), "LLM_INVALID_KEY");
        let msg = err.user_message();
        assert!(msg.contains("openai"), "user_message 必须包含 provider_id");
        assert!(
            msg.contains("API Key") || msg.contains("Key"),
            "user_message 必须明确提示 Key 问题"
        );

        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":\"LLM_INVALID_KEY\""));
        assert!(json.contains("\"provider_id\":\"openai\""));
    }

    /// 回归：过长 detail 截断且不带控制字符（防止 XSS / 终端控制码）
    #[test]
    fn llm_invalid_key_truncates_and_strips_control_chars() {
        let detail = "a".repeat(500) + "\n\x07\x1b[31mINJECTED\x1b[0m";
        let err = AppError::LlmInvalidKey {
            provider_id: "p".to_string(),
            detail,
        };
        let msg = err.user_message();
        assert!(!msg.contains('\x07'));
        assert!(!msg.contains('\x1b'));
        assert!(msg.contains('…'), "超长 detail 必须有省略号标记");
        // 不能超过约 120 字符 + provider 前缀
        assert!(msg.chars().count() < 200);
    }

    /// 回归：classify_llm_error 必须正确区分 401 / 普通错误
    #[test]
    fn classify_llm_error_routes_401_to_structured_variant() {
        for sample in &[
            "OpenAI API error (401): Incorrect API key provided",
            "Anthropic API error (401 Unauthorized): invalid x-api-key",
            "Authentication failed: API key not configured",
            "401 unauthorized: missing credentials",
        ] {
            let err = AppError::classify_llm_error("openai", sample);
            assert!(
                matches!(err, AppError::LlmInvalidKey { .. }),
                "应当归类为 LlmInvalidKey，但得到: {:?}（输入: {}）",
                err,
                sample
            );
        }
    }

    #[test]
    fn classify_llm_error_routes_other_errors_to_api() {
        for sample in &[
            "LLM 调用超时",
            "OpenAI API error (500): internal error",
            "网络连接失败",
        ] {
            let err = AppError::classify_llm_error("openai", sample);
            assert!(
                matches!(err, AppError::Api(_)),
                "非 401 错误应保持 Api 变体: {:?}（输入: {}）",
                err,
                sample
            );
        }
    }
}
