//! 工具约束 + Ping-Pong 检测
//!
//! 防止 LLM 在同一执行步骤中重复调用相同工具（Ping-Pong 循环），
//! 以及检测其他工具使用模式违规。

use std::collections::{HashMap, HashSet};

/// 工具调用约束检查器
pub struct ToolConstraintChecker {
    /// 每个步骤的工具调用历史
    call_history: HashMap<String, Vec<String>>,
    /// Ping-Pong 检测：记录工具调用的归一化键
    seen_keys: HashSet<String>,
    /// 最大连续相同调用次数
    max_identical_calls: usize,
}

impl ToolConstraintChecker {
    pub fn new() -> Self {
        Self {
            call_history: HashMap::new(),
            seen_keys: HashSet::new(),
            max_identical_calls: 3,
        }
    }

    /// 归一化工具调用键（名称 + 关键参数哈希）
    pub fn normalized_call_key(tool_name: &str, args: &str) -> String {
        // 简单归一化：工具名 + 参数的前 100 字符
        let args_preview = if args.len() > 100 {
            let mut end = 100;
            while end > 0 && !args.is_char_boundary(end) {
                end -= 1;
            }
            &args[..end]
        } else {
            args
        };
        format!("{}:{}", tool_name, args_preview)
    }

    /// 检查工具调用是否被允许
    /// 返回 None 表示允许，Some(ConstraintViolation) 表示违规
    pub fn check_call(
        &mut self,
        step_id: &str,
        tool_name: &str,
        args: &str,
    ) -> Option<ConstraintViolation> {
        let key = Self::normalized_call_key(tool_name, args);

        // Ping-Pong 检测
        let count = self.seen_keys.iter().filter(|k| **k == key).count();
        if count >= self.max_identical_calls {
            return Some(ConstraintViolation::PingPongDetected {
                tool: tool_name.to_string(),
                call_count: count + 1,
                max_allowed: self.max_identical_calls,
            });
        }

        // 记录调用
        self.call_history
            .entry(step_id.to_string())
            .or_default()
            .push(tool_name.to_string());
        self.seen_keys.insert(key);

        None
    }

    /// 重置步骤约束（每个新步骤调用一次）
    pub fn reset_for_step(&mut self) {
        self.seen_keys.clear();
    }
}

/// 约束违规
#[derive(Debug, Clone)]
pub enum ConstraintViolation {
    /// Ping-Pong 检测：同一工具被重复调用
    PingPongDetected {
        tool: String,
        call_count: usize,
        max_allowed: usize,
    },
    /// 工具不在白名单中
    ToolNotInWhitelist {
        tool: String,
        allowed_tools: Vec<String>,
    },
}

impl std::fmt::Display for ConstraintViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PingPongDetected {
                tool,
                call_count,
                max_allowed,
            } => {
                write!(
                    f,
                    "工具 '{}' 被连续调用 {} 次，超过上限 {}（Ping-Pong 检测）",
                    tool, call_count, max_allowed
                )
            }
            Self::ToolNotInWhitelist {
                tool,
                allowed_tools,
            } => {
                write!(
                    f,
                    "工具 '{}' 不在允许列表中。允许的工具: {:?}",
                    tool, allowed_tools
                )
            }
        }
    }
}

/// 强制执行工具约束
pub fn enforce_tool_constraint(
    checker: &mut ToolConstraintChecker,
    step_id: &str,
    tool_name: &str,
    args: &str,
    allowed_tools: Option<&[String]>,
) -> Result<(), ConstraintViolation> {
    // 白名单检查
    if let Some(whitelist) = allowed_tools {
        if !whitelist.contains(&tool_name.to_string()) {
            return Err(ConstraintViolation::ToolNotInWhitelist {
                tool: tool_name.to_string(),
                allowed_tools: whitelist.to_vec(),
            });
        }
    }

    // Ping-Pong 检测
    if let Some(violation) = checker.check_call(step_id, tool_name, args) {
        return Err(violation);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_pong_detection() {
        let mut checker = ToolConstraintChecker::new();
        let args = r#"{"query": "金蝶"}"#;

        // 前 3 次允许
        assert!(checker
            .check_call("step1", "search-knowledge", args)
            .is_none());
        assert!(checker
            .check_call("step1", "search-knowledge", args)
            .is_none());
        assert!(checker
            .check_call("step1", "search-knowledge", args)
            .is_none());

        // 第 4 次触发 Ping-Pong
        let violation = checker.check_call("step1", "search-knowledge", args);
        assert!(matches!(
            violation,
            Some(ConstraintViolation::PingPongDetected { .. })
        ));
    }

    #[test]
    fn test_different_args_allowed() {
        let mut checker = ToolConstraintChecker::new();
        assert!(checker
            .check_call("step1", "search-knowledge", r#"{"query": "A"}"#)
            .is_none());
        assert!(checker
            .check_call("step1", "search-knowledge", r#"{"query": "B"}"#)
            .is_none());
    }

    #[test]
    fn test_whitelist_enforcement() {
        let mut checker = ToolConstraintChecker::new();
        let whitelist = vec!["search-knowledge".to_string(), "generate-doc".to_string()];

        let result = enforce_tool_constraint(
            &mut checker,
            "step1",
            "search-knowledge",
            "{}",
            Some(&whitelist),
        );
        assert!(result.is_ok());

        let result = enforce_tool_constraint(
            &mut checker,
            "step1",
            "unknown-tool",
            "{}",
            Some(&whitelist),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_normalized_call_key() {
        let key1 = ToolConstraintChecker::normalized_call_key("search", r#"{"q": "test"}"#);
        let key2 = ToolConstraintChecker::normalized_call_key("search", r#"{"q": "test"}"#);
        let key3 = ToolConstraintChecker::normalized_call_key("search", r#"{"q": "other"}"#);
        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }
}
