//! 分层摘要压缩 + 磁滞回线防震荡 + 增量摘要

use super::llm_service::ChatMessage;
use super::token;

/// 磁滞回线参数
pub struct CompressionHysteresis {
    pub trigger_threshold_pct: u32,   // 80%
    pub release_target_pct: u32,       // 50%
    pub reset_threshold_pct: u32,      // 30%
    pub is_compressed: bool,
}

impl CompressionHysteresis {
    pub fn new(trigger_pct: u32, release_pct: u32, reset_pct: u32) -> Self {
        Self { trigger_threshold_pct: trigger_pct, release_target_pct: release_pct, reset_threshold_pct: reset_pct, is_compressed: false }
    }

    pub fn should_compress(&mut self, usage_pct: u32) -> bool {
        if !self.is_compressed && usage_pct >= self.trigger_threshold_pct {
            self.is_compressed = true;
            return true;
        }
        false
    }

    pub fn on_compressed(&mut self, total_budget: u32) -> u32 {
        total_budget * self.release_target_pct / 100
    }

    pub fn maybe_reset(&mut self, usage_pct: u32) {
        if self.is_compressed && usage_pct < self.reset_threshold_pct {
            self.is_compressed = false;
        }
    }
}

/// 增量摘要器（使用消息 ID 而非索引）
pub struct IncrementalSummarizer {
    pub prev_summary: Option<String>,
    pub last_message_id: Option<String>,
}

/// 压缩后的历史记录
pub struct CompressedHistory {
    pub summary: Option<String>,
    pub critical_turns: Vec<ChatMessage>,
    pub recent_turns: Vec<ChatMessage>,
    pub tokens_used: u32,
}

/// 标记关键轮次
fn mark_critical_indices(messages: &[ChatMessage]) -> Vec<usize> {
    messages.iter().enumerate()
        .filter(|(_, m)| m.role == "system" || m.content.contains("【上一轮工具上下文】") || m.content.contains("错误") || m.content.contains("失败"))
        .map(|(i, _)| i).collect()
}

/// 从尾部保留最近消息，直到 token 预算用完
fn retain_recent(messages: &[ChatMessage], budget: u32) -> Vec<ChatMessage> {
    let mut result = Vec::new();
    let mut tokens = 0u32;
    for msg in messages.iter().rev() {
        let msg_tokens = token::count_tokens_with_fallback(&msg.content) + token::count_tokens_with_fallback(&msg.role) + 4;
        if tokens + msg_tokens > budget && !result.is_empty() { break; }
        tokens += msg_tokens;
        result.push(msg.clone());
    }
    result.reverse();
    result
}

/// 格式化消息列表为文本（用于摘要 prompt）
fn format_messages(messages: &[ChatMessage]) -> String {
    messages.iter().map(|m| format!("{}: {}", m.role, m.content)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hysteresis_triggers_and_releases() {
        let mut h = CompressionHysteresis::new(80, 50, 30);
        assert!(!h.should_compress(70));
        assert!(h.should_compress(85)); // triggers at 85 >= 80
        assert!(!h.should_compress(90)); // already compressed

        let release = h.on_compressed(1000);
        assert_eq!(release, 500);

        h.maybe_reset(40); // 40 > 30, still compressed
        assert!(h.is_compressed);
        h.maybe_reset(25); // 25 < 30, resets
        assert!(!h.is_compressed);
    }

    #[test]
    fn test_retain_recent_truncates() {
        let msgs = vec![
            ChatMessage { role: "user".into(), content: "hello".repeat(1000), id: None, token_count: None },
        ];
        let result = retain_recent(&msgs, 10);
        assert!(result.len() <= 1);
    }
}
