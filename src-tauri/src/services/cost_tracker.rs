//! Token 用量统计和成本追踪
//!
//! 追踪 LLM 调用的 token 消耗和工具调用次数，用于成本监控。

use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

/// Token 用量追踪器
///
/// 使用原子计数器，线程安全，可在 agent 会话间共享。
#[derive(Debug, Default)]
pub struct CostTracker {
    pub total_input_tokens: AtomicU64,
    pub total_output_tokens: AtomicU64,
    pub total_tool_calls: AtomicU64,
    pub total_llm_calls: AtomicU64,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次 LLM 调用的 token 用量
    pub fn record_llm_call(&self, input_tokens: u64, output_tokens: u64) {
        self.total_input_tokens.fetch_add(input_tokens, Ordering::Relaxed);
        self.total_output_tokens.fetch_add(output_tokens, Ordering::Relaxed);
        self.total_llm_calls.fetch_add(1, Ordering::Relaxed);

        info!(
            target: "cost",
            input_tokens,
            output_tokens,
            total_input = self.total_input_tokens.load(Ordering::Relaxed),
            total_output = self.total_output_tokens.load(Ordering::Relaxed),
            "LLM call recorded"
        );
    }

    /// 记录一次工具调用
    pub fn record_tool_call(&self) {
        self.total_tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    /// 获取用量摘要
    pub fn summary(&self) -> CostSummary {
        CostSummary {
            total_input_tokens: self.total_input_tokens.load(Ordering::Relaxed),
            total_output_tokens: self.total_output_tokens.load(Ordering::Relaxed),
            total_tool_calls: self.total_tool_calls.load(Ordering::Relaxed),
            total_llm_calls: self.total_llm_calls.load(Ordering::Relaxed),
        }
    }
}

/// Token 用量摘要（可序列化，用于前端展示）
#[derive(Debug, Clone, serde::Serialize)]
pub struct CostSummary {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tool_calls: u64,
    pub total_llm_calls: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_llm_call() {
        let tracker = CostTracker::new();
        tracker.record_llm_call(100, 50);
        tracker.record_llm_call(200, 80);

        let summary = tracker.summary();
        assert_eq!(summary.total_input_tokens, 300);
        assert_eq!(summary.total_output_tokens, 130);
        assert_eq!(summary.total_llm_calls, 2);
    }

    #[test]
    fn test_record_tool_call() {
        let tracker = CostTracker::new();
        tracker.record_tool_call();
        tracker.record_tool_call();
        tracker.record_tool_call();

        let summary = tracker.summary();
        assert_eq!(summary.total_tool_calls, 3);
    }

    #[test]
    fn test_summary_default() {
        let tracker = CostTracker::new();
        let summary = tracker.summary();
        assert_eq!(summary.total_input_tokens, 0);
        assert_eq!(summary.total_output_tokens, 0);
        assert_eq!(summary.total_tool_calls, 0);
        assert_eq!(summary.total_llm_calls, 0);
    }
}
