//! 信号写入器 — 记录技能系统事件到 signals.jsonl
//!
//! 参考 Claude Code 技能系统的事件流机制：
//!   - JSONL 格式追加写入
//!   - 事件类型包括：技能加载、匹配、执行、错误等
//!   - 支持时间戳和会话 ID

use std::io::Write;
use std::path::PathBuf;

/// 信号事件
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SignalEvent {
    pub event_type: String,
    pub skill_id: String,
    pub timestamp: String,
    pub session_id: Option<String>,
    pub data: serde_json::Value,
}

/// 信号写入器
pub struct SignalWriter {
    /// 信号文件路径
    signals_path: PathBuf,
}

impl SignalWriter {
    /// 创建信号写入器
    pub fn new(signals_path: PathBuf) -> Result<Self, std::io::Error> {
        // 确保目录存在
        if let Some(parent) = signals_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(Self { signals_path })
    }

    /// 写入信号事件
    pub fn write(&self, event: SignalEvent) -> Result<(), std::io::Error> {
        let json = serde_json::to_string(&event)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.signals_path)?;
        writeln!(file, "{}", json)?;
        Ok(())
    }

    /// 读取所有信号事件
    pub fn read_all(&self) -> Result<Vec<SignalEvent>, std::io::Error> {
        if !self.signals_path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.signals_path)?;
        let events = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        Ok(events)
    }

    /// 读取最近的 N 条事件
    pub fn read_recent(&self, count: usize) -> Result<Vec<SignalEvent>, std::io::Error> {
        let all = self.read_all()?;
        let start = if all.len() > count { all.len() - count } else { 0 };
        Ok(all[start..].to_vec())
    }

    /// 清空信号文件
    pub fn clear(&self) -> Result<(), std::io::Error> {
        std::fs::write(&self.signals_path, "")?;
        Ok(())
    }
}

/// 事件类型常量
pub mod event_types {
    pub const SKILL_LOADED: &str = "skill_loaded";
    pub const SKILL_MATCHED: &str = "skill_matched";
    pub const SKILL_EXECUTED: &str = "skill_executed";
    pub const SKILL_ERROR: &str = "skill_error";
    pub const TEMPLATE_DOWNLOADED: &str = "template_downloaded";
    pub const TEMPLATE_CACHE_HIT: &str = "template_cache_hit";
}

/// 创建信号事件的便捷函数
impl SignalEvent {
    pub fn skill_loaded(skill_id: &str, load_time_ms: u64) -> Self {
        Self {
            event_type: event_types::SKILL_LOADED.to_string(),
            skill_id: skill_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: None,
            data: serde_json::json!({
                "load_time_ms": load_time_ms,
            }),
        }
    }

    pub fn skill_matched(skill_id: &str, score: f64, match_type: &str) -> Self {
        Self {
            event_type: event_types::SKILL_MATCHED.to_string(),
            skill_id: skill_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: None,
            data: serde_json::json!({
                "score": score,
                "match_type": match_type,
            }),
        }
    }

    pub fn skill_executed(skill_id: &str, success: bool, duration_ms: u64) -> Self {
        Self {
            event_type: event_types::SKILL_EXECUTED.to_string(),
            skill_id: skill_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: None,
            data: serde_json::json!({
                "success": success,
                "duration_ms": duration_ms,
            }),
        }
    }

    pub fn skill_error(skill_id: &str, error: &str) -> Self {
        Self {
            event_type: event_types::SKILL_ERROR.to_string(),
            skill_id: skill_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: None,
            data: serde_json::json!({
                "error": error,
            }),
        }
    }

    pub fn template_downloaded(template_id: &str, path: &str) -> Self {
        Self {
            event_type: event_types::TEMPLATE_DOWNLOADED.to_string(),
            skill_id: "template_manager".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: None,
            data: serde_json::json!({
                "template_id": template_id,
                "path": path,
            }),
        }
    }

    pub fn template_cache_hit(template_id: &str) -> Self {
        Self {
            event_type: event_types::TEMPLATE_CACHE_HIT.to_string(),
            skill_id: "template_manager".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: None,
            data: serde_json::json!({
                "template_id": template_id,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_event_serialization() {
        let event = SignalEvent::skill_loaded("test-skill", 100);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("skill_loaded"));
        assert!(json.contains("test-skill"));
    }

    #[test]
    fn test_signal_writer_write_and_read() {
        let temp_dir = tempfile::tempdir().unwrap();
        let signals_path = temp_dir.path().join("signals.jsonl");

        let writer = SignalWriter::new(signals_path.clone()).unwrap();

        // 写入事件
        let event1 = SignalEvent::skill_loaded("skill1", 50);
        let event2 = SignalEvent::skill_matched("skill2", 0.85, "keyword");
        writer.write(event1).unwrap();
        writer.write(event2).unwrap();

        // 读取事件
        let events = writer.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].skill_id, "skill1");
        assert_eq!(events[1].skill_id, "skill2");

        // 读取最近 1 条
        let recent = writer.read_recent(1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].skill_id, "skill2");
    }
}
