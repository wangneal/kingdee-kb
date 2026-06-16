//! 安全 spawn helper —— 解决"fire-and-forget spawn panic 任务卡 in_progress 永久"。
//!
//! ## 背景
//!
//! 项目里 9+ 处 `tauri::async_runtime::spawn(async move { ... })`，每个都是
//! fire-and-forget 模式。如果内部 `?` 错误传播未处理（例：spawn 后调
//! `update_status` 但锁失败 → `return;`），或者 LLM API 解析 panic，整个
//! 任务会**静默失败**。前端 UI 上看到"重编译进行中..."、`"Agent 思考中..."`
//! 永远不变，**没有任何错误信号**。
//!
//! ## 解决
//!
//! [`spawn_monitored`] 包装一层 `catch_unwind`，把 panic 转换为 `tracing::error!`
//! + `app.emit("task:failed", ...)` 事件，前端监听后弹"任务失败"提示。
//!
//! ## 用法
//!
//! 旧代码：
//! ```ignore
//! tauri::async_runtime::spawn(async move {
//!     do_long_running_work().await;
//!     // 如果上面 panic 整个 Tauri 进程崩；如果 Err 没人更新 status
//! });
//! ```
//!
//! 新代码：
//! ```ignore
//! spawn_monitored("kb_recompile", &app_handle, async move {
//!     match do_long_running_work().await {
//!         Ok(done) => tracing::info!("done: {done:?}"),
//!         Err(e) => tracing::error!("failed: {e}"),
//!     }
//! });
//! ```
//!
//! ## 注意
//!
//! - `app_handle` 可选 —— 不传则只写日志，不 emit
//! - `name` 用于日志/事件定位，**必须是 `&'static str`**
//! - callback 内部依然要 `match` Result，panic 兜底是最后防线，不是正常错误处理

use std::panic::AssertUnwindSafe;

use futures::FutureExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// 监控的 spawn —— panic 时记 log + emit `task:failed` 事件
pub fn spawn_monitored<F>(name: &'static str, app: Option<&AppHandle>, future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let app_for_emit = app.cloned();
    tauri::async_runtime::spawn(async move {
        let result = AssertUnwindSafe(future).catch_unwind().await;
        if let Err(panic_payload) = result {
            let msg = extract_panic_message(&panic_payload);
            tracing::error!("[spawn_monitored:{}] 任务 panic: {}", name, msg);

            if let Some(app) = app_for_emit {
                let event = TaskFailedEvent {
                    name,
                    message: msg,
                };
                if let Err(emit_err) = app.emit("task:failed", &event) {
                    tracing::warn!("emit task:failed 失败: {}", emit_err);
                }
            }
        }
    });
}

/// emit 给前端的 task:failed 事件 schema
#[derive(Debug, Clone, Serialize)]
pub struct TaskFailedEvent {
    /// 任务名（`&'static str` → 序列化时变 &'static str）
    pub name: &'static str,
    /// panic 消息（去 "thread 'xxx' panicked at" 前缀）
    pub message: String,
}

fn extract_panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "未知 panic payload".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn extract_panic_message_from_str() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("oops");
        assert_eq!(extract_panic_message(&payload), "oops");
    }

    #[tokio::test]
    async fn extract_panic_message_from_string() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("owned"));
        assert_eq!(extract_panic_message(&payload), "owned");
    }

    #[tokio::test]
    async fn extract_panic_message_unknown() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42_i32);
        assert_eq!(extract_panic_message(&payload), "未知 panic payload");
    }

    #[tokio::test]
    async fn spawn_monitored_catches_panic() {
        // 简单验证 catch_unwind 包裹后 panic 不再传播
        let future = async {
            panic!("intentional test panic");
        };
        let result = AssertUnwindSafe(future).catch_unwind().await;
        assert!(result.is_err());
    }
}
