//! Agent 失败日志 — 驾驭工程的"活文档"机制
//!
//! 每当 Agent 连续失败或触发重新规划时，记录失败模式。
//! 这些记录会在下次会话时注入系统提示词，防止同类错误重演。
//!
//! 文件存储在 `{data_dir}/agents_failures.json`，跨会话持久化。

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 单条失败记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    /// 失败时间（Unix 时间戳）
    pub timestamp: u64,
    /// 失败类型：exhausted / replan / timeout
    pub failure_type: String,
    /// 失败原因摘要
    pub reason: String,
    /// 会话 ID
    pub session_id: String,
    /// 从中提炼的约束规则（人类可读）
    pub derived_rule: String,
}

/// 失败日志管理器
pub struct AgentsLog {
    /// 日志文件路径
    log_path: PathBuf,
    /// 最大保留条数（避免提示词膨胀）
    max_records: usize,
    /// 内存缓存（VecDeque 避免首部删除 O(n)）
    records: VecDeque<FailureRecord>,
}

impl AgentsLog {
    /// 创建日志管理器，自动从磁盘加载历史记录
    pub fn new(data_dir: &PathBuf) -> Self {
        let log_path = data_dir.join("agents_failures.json");
        let records: VecDeque<FailureRecord> = Self::load_from_disk(&log_path).into();
        Self {
            log_path,
            max_records: 20,
            records,
        }
    }

    /// 记录一次 Agent 失败
    ///
    /// 自动去重（相同 `failure_type + reason` 不重复记录），
    /// 超过 `max_records` 时淘汰最旧的记录。
    pub fn record_failure(&mut self, failure_type: &str, reason: &str, session_id: &str) {
        // 去重：检查是否已有相同模式
        let dedup_key = format!("{}:{}", failure_type, reason);
        if self
            .records
            .iter()
            .any(|r| format!("{}:{}", r.failure_type, r.reason) == dedup_key)
        {
            tracing::debug!("Agent 失败模式已存在，跳过: {}", dedup_key);
            return;
        }

        let derived_rule = Self::derive_rule(failure_type, reason);

        let record = FailureRecord {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            failure_type: failure_type.to_string(),
            reason: reason.to_string(),
            session_id: session_id.to_string(),
            derived_rule,
        };

        tracing::info!(
            failure_type = failure_type,
            reason = reason,
            "Agent 失败已记录，提炼规则: {}",
            record.derived_rule
        );

        self.records.push_back(record);

        // 淘汰最旧的记录
        while self.records.len() > self.max_records {
            self.records.pop_front();
        }

        // 持久化到磁盘
        self.save_to_disk();
    }

    /// 获取用于注入系统提示词的约束规则列表
    ///
    /// 返回最近 N 条从失败中提炼的规则，格式为提示词可嵌入的文本。
    pub fn get_learned_constraints(&self) -> Option<String> {
        if self.records.is_empty() {
            return None;
        }

        let rules: Vec<&str> = self
            .records
            .iter()
            .rev()
            .take(10)
            .map(|r| r.derived_rule.as_str())
            .collect();

        let mut section = String::from("【历史教训 — 以下规则来自 Agent 过往失败，必须遵守】\n");
        for (i, rule) in rules.iter().enumerate() {
            section.push_str(&format!("{}. {}\n", i + 1, rule));
        }
        section.push('\n');

        Some(section)
    }

    /// 获取所有记录（用于前端展示）
    pub fn records(&self) -> &VecDeque<FailureRecord> {
        &self.records
    }

    /// 清空所有记录
    pub fn clear(&mut self) {
        self.records.clear();
        self.save_to_disk();
    }

    /// 从失败信息中提炼约束规则
    fn derive_rule(failure_type: &str, reason: &str) -> String {
        match failure_type {
            "exhausted" => {
                // 连续失败 → 提取工具使用约束
                if reason.contains("为空") || reason.contains("empty") {
                    "调用工具后必须检查结果是否为空，空结果不应继续后续流程".to_string()
                } else if reason.contains("超时") || reason.contains("timeout") {
                    "工具调用超时时必须降级处理，不要反复重试同一操作".to_string()
                } else if reason.contains("不支持") || reason.contains("not supported") {
                    "遇到不支持的功能时，应立即告知用户而非反复尝试".to_string()
                } else {
                    format!("连续失败时需检查原因: {}", truncate(reason, 80))
                }
            }
            "replan" => {
                // 重新规划 → 提取流程约束
                format!("任务执行受阻时应重新评估方案: {}", truncate(reason, 80))
            }
            "timeout" => "单次工具调用超时时，应切换备选方案而非无限等待".to_string(),
            _ => {
                format!("注意: {}", truncate(reason, 80))
            }
        }
    }

    fn load_from_disk(path: &PathBuf) -> Vec<FailureRecord> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    fn save_to_disk(&self) {
        if let Ok(data) = serde_json::to_string_pretty(&self.records) {
            let _ = std::fs::write(&self.log_path, data);
        }
    }
}

/// 截断字符串到指定长度
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_rule_exhausted_empty() {
        let rule = AgentsLog::derive_rule("exhausted", "步骤结果为空");
        assert!(rule.contains("为空"));
    }

    #[test]
    fn test_derive_rule_timeout() {
        let rule = AgentsLog::derive_rule("timeout", "操作超时");
        assert!(rule.contains("备选方案"));
    }

    #[test]
    fn test_dedup() {
        let dir = std::env::temp_dir().join("agents_log_test_dedup");
        let _ = std::fs::create_dir_all(&dir);
        let mut log = AgentsLog::new(&dir);
        log.record_failure("exhausted", "连续失败", "s1");
        log.record_failure("exhausted", "连续失败", "s2"); // 重复
        assert_eq!(log.records.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_learned_constraints() {
        let dir = std::env::temp_dir().join("agents_log_test_constraints");
        let _ = std::fs::create_dir_all(&dir);
        let mut log = AgentsLog::new(&dir);
        assert!(log.get_learned_constraints().is_none());
        log.record_failure("exhausted", "步骤结果为空", "s1");
        let constraints = log.get_learned_constraints().unwrap();
        assert!(constraints.contains("历史教训"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
