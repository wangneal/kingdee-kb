//! Agent 超时和重试配置
//!
//! 集中管理所有超时常量，便于调优和测试。

use std::time::Duration;

/// LLM API 非流式调用超时（秒）
pub const LLM_CALL_TIMEOUT_SECS: u64 = 120;

/// LLM 流式调用首 chunk 超时（秒）— 从请求发出到收到首个数据块
pub const LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS: u64 = 30;

/// LLM 流式调用总超时（秒）— 整个流式会话的最大时长
pub const LLM_STREAM_TOTAL_TIMEOUT_SECS: u64 = 300;

/// 工具执行超时（秒）
pub const TOOL_EXECUTION_TIMEOUT_SECS: u64 = 120;

/// Agent 总会话超时（秒）— 从用户发消息到 agent 完成
pub const AGENT_SESSION_TIMEOUT_SECS: u64 = 600; // 10 分钟

/// 用户回答澄清问题的超时（秒）— 与 rig_tool.rs 中的常量保持一致
pub const QUESTION_TIMEOUT_SECS: u64 = 300; // 5 分钟

/// 工具调用最大重试次数
pub const MAX_RETRIES: u32 = 3;

/// 重试基础延迟（毫秒）
pub const RETRY_BASE_DELAY_MS: u64 = 1000;

/// 计算指数退避延迟（第 N 次重试的等待时间）
pub fn retry_delay(attempt: u32) -> Duration {
    Duration::from_millis(RETRY_BASE_DELAY_MS * 2u64.pow(attempt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_delay_exponential() {
        let d0 = retry_delay(0);
        let d1 = retry_delay(1);
        let d2 = retry_delay(2);

        assert_eq!(d0, Duration::from_millis(1000));
        assert_eq!(d1, Duration::from_millis(2000));
        assert_eq!(d2, Duration::from_millis(4000));
    }

    #[test]
    fn test_timeout_constants_sane() {
        assert!(LLM_CALL_TIMEOUT_SECS > 0);
        assert!(LLM_STREAM_FIRST_CHUNK_TIMEOUT_SECS > 0);
        assert!(AGENT_SESSION_TIMEOUT_SECS > LLM_CALL_TIMEOUT_SECS);
        assert!(QUESTION_TIMEOUT_SECS > 0);
        assert!(MAX_RETRIES > 0);
    }
}
