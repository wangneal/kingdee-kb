//! 全局错误类型定义。
//!
//! ## 设计原则
//!
//! 1. **统一入口**：所有公开的内部函数应返回 [`AppResult<T>`]，避免在调用链
//!    中散落 `Result<_, String>` / `Result<_, Box<dyn Error>>`。
//! 2. **结构化错误**：[`AppError`] 是 `thiserror` 派生的 enum，每个变体代表
//!    一类可被区分的失败，调用方可以 `match` 决定重试 / 降级 / 提示用户。
//! 3. **可携带上下文**：通过 [`anyhow::Context`] 在调用点追加上下文（"无法
//!    打开 foo.json"），方便日志聚合和调试。
//! 4. **Tauri 兼容**：[`AppError`] 实现了 `Serialize`，可以原样作为
//!    `#[tauri::command]` 的返回值传给前端（前端会收到一个结构化对象，
//!    而非纯字符串）。
//!
//! ## 迁移指南
//!
//! 旧代码：
//! ```ignore
//! fn load_config() -> Result<Config, String> {
//!     fs::read_to_string("config.json").map_err(|e| e.to_string())
//! }
//! ```
//!
//! 新代码：
//! ```ignore
//! fn load_config() -> AppResult<Config> {
//!     let content = fs::read_to_string("config.json")
//!         .context("读取 config.json 失败")?;
//!     let cfg: Config = toml::from_str(&content)
//!         .context("解析 config.json 失败")?;
//!     Ok(cfg)
//! }
//! ```

use std::path::PathBuf;

use serde::{Serialize, Serializer};
use thiserror::Error;

/// 应用统一错误类型。
///
/// 新增变体时请同步：
/// 1. 在 `From` impl 块里加 `#[from]` 转换（若适用）
/// 2. 更新 [`AppError::code`] 用于前端分类
/// 3. 更新 [`AppError::user_message`] 用于 UI 友好展示
#[derive(Error, Debug)]
pub enum AppError {
    /// SQLite / rusqlite 错误
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    /// 文件 IO 错误
    #[error("IO 错误 ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// JSON 序列化 / 反序列化
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),

    /// 通用配置错误（路径不存在、字段缺失等）
    #[error("配置错误: {0}")]
    Config(String),

    /// API 调用错误（LLM / Whisper / Embedding）
    #[error("API 错误: {0}")]
    Api(String),

    /// LLM API Key 401 / 无效。
    ///
    /// 与 `Api` 区分的原因：401 是**用户**操作错误（Key 失效、过期、配错）
    /// 而不是上游服务问题，前端需要用**专门的对话框**引导用户去设置页修改 API Key，
    /// 而不是显示"重试"按钮。
    ///
    /// 触发场景：
    /// - HTTP 401 + body 包含 "unauthorized" / "invalid api key" / "incorrect api key"
    /// - 全部可用 Key 故障切换耗尽
    #[error("LLM API Key 无效（供应商: {provider_id}）")]
    LlmInvalidKey {
        provider_id: String,
        /// 原始服务端响应（已截断到 200 字），帮助用户诊断 Key 失效原因
        detail: String,
    },

    /// 用户输入校验失败
    #[error("参数错误: {0}")]
    InvalidArgument(String),

    /// 资源未找到
    #[error("未找到: {0}")]
    NotFound(String),

    /// 内部状态不一致（数据库约束违反、不变量被破坏）
    #[error("内部错误: {0}")]
    Internal(String),

    /// 上游 anyhow 错误透传（用 `.context()` 加的上下文）
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
}

/// 应用统一 Result 别名
pub type AppResult<T> = Result<T, AppError>;

// ── Tauri IPC 序列化 ───────────────────────────────────────────────────────

/// Tauri command 必须能 `serde::Serialize`，前端拿到的是结构化对象：
/// ```json
/// { "code": "DATABASE", "message": "database is locked" }
/// ```
///
/// `LlmInvalidKey` 额外携带 `provider_id`，方便前端直接定位到对应供应商配置。
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
    let trimmed: String = text.chars().filter(|c| !c.is_control()).collect();
    if trimmed.chars().count() <= max_chars {
        trimmed
    } else {
        let mut out: String = trimmed.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

// ── std::io::Error 的便捷 From ─────────────────────────────────────────────

impl AppError {
    /// 包装 IO 错误并附加路径上下文
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        AppError::Io {
            path: path.into(),
            source,
        }
    }

    /// 把 LLM 内部 `String` 错误归类为 [`AppError::LlmInvalidKey`] 或 [`AppError::Api`]。
    ///
    /// LLM 服务内部仍以 `String` 传错误（保持向后兼容），但命令层在 Tauri IPC 之前
    /// 调一次 `classify_llm_error`，把 401 / 失效 Key 升级为结构化错误，
    /// 让前端能区分"重试一下"和"需要去设置页换 Key"。
    ///
    /// 归类规则（与 `llm_service::is_auth_error` 保持一致）：
    /// - 包含 `401` / `unauthorized` / `invalid api key` / `incorrect api key` →
    ///   `LlmInvalidKey`
    /// - 其他 → `Api`（透传）
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

// ── 测试 ──────────────────────────────────────────────────────────────────

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
        // user message is meant for end-users, don't expose low-level detail
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

    /// 回归：LlmInvalidKey 必须能被前端 switch 识别，且序列化要带 provider_id
    ///
    /// 修复前：401 / Invalid API Key 只是普通字符串错误，
    ///        前端只看到 "外部服务调用失败" 之类的通用 toast，
    ///        用户必须自己联想到要去设置页查 API Key
    /// 修复后：code = LLM_INVALID_KEY，message 明确指向供应商 + 详情，
    ///        前端弹出"配置 API Key"对话框，一键跳转设置页
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
