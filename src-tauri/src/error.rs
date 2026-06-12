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
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.user_message())?;
        s.end()
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
}
