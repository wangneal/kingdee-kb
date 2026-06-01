//! 熵管理器 — 技术债务清理和文档一致性维护
//!
//! 扫描过期技能、文档-代码不一致、向量索引漂移。
//! 所有清理操作需用户确认，不自动执行。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 过期项
#[derive(Debug, Clone)]
pub struct StaleItem {
    pub path: PathBuf,
    pub last_accessed_days: u64,
    pub item_type: StaleType,
}

/// 过期类型
#[derive(Debug, Clone, PartialEq)]
pub enum StaleType {
    /// 技能文件超过 90 天未使用
    Skill,
    /// 文档与代码不一致
    DocMismatch,
    /// 向量索引与源文件不一致
    IndexDrift,
}

/// 文档-代码不一致项
#[derive(Debug, Clone)]
pub struct DocMismatch {
    pub doc_path: PathBuf,
    pub description: String,
    pub severity: MismatchSeverity,
}

/// 不一致严重程度
#[derive(Debug, Clone, PartialEq)]
pub enum MismatchSeverity {
    /// 文档中引用的文件不存在
    BrokenReference,
    /// 文档描述与实际行为不符
    DescriptionDrift,
    /// 文档过时但无害
    Outdated,
}

/// 索引漂移项
#[derive(Debug, Clone)]
pub struct IndexDrift {
    pub source_path: PathBuf,
    pub stored_hash: String,
    pub current_hash: String,
}

/// 清理项（用户确认后执行）
#[derive(Debug, Clone)]
pub struct CleanupItem {
    pub item: StaleItem,
    pub action: CleanupAction,
}

/// 清理动作
#[derive(Debug, Clone, PartialEq)]
pub enum CleanupAction {
    /// 删除过期技能
    Remove,
    /// 更新文档
    UpdateDoc,
    /// 重新索引
    Reindex,
}

/// 清理报告
#[derive(Debug, Clone, Default)]
pub struct CleanupReport {
    pub removed_skills: usize,
    pub updated_docs: usize,
    pub reindexed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

/// 熵管理器
pub struct EntropyManager {
    data_dir: PathBuf,
    /// 技能过期阈值（天）
    stale_threshold_days: u64,
}

impl EntropyManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            stale_threshold_days: 90,
        }
    }

    /// 获取数据目录
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// 扫描目录中的过期文件
    ///
    /// 遍历指定子目录，检查文件修改时间是否超过阈值。
    /// 不执行任何删除操作。
    pub fn scan_stale_files(&self, subdir: &str) -> Vec<StaleItem> {
        let scan_dir = self.data_dir.join(subdir);
        if !scan_dir.exists() {
            return Vec::new();
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let _threshold_secs = self.stale_threshold_days * 86400;
        let mut stale_items = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&scan_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                if let Ok(metadata) = path.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                            let age_secs = now.saturating_sub(duration.as_secs());
                            let age_days = age_secs / 86400;

                            if age_days > self.stale_threshold_days {
                                stale_items.push(StaleItem {
                                    path: path.clone(),
                                    last_accessed_days: age_days,
                                    item_type: StaleType::Skill,
                                });
                            }
                        }
                    }
                }
            }
        }

        tracing::info!(
            subdir = subdir,
            found = stale_items.len(),
            "过期文件扫描完成"
        );

        stale_items
    }

    /// 计算文件内容的简单哈希（用于索引漂移检测）
    ///
    /// 使用简单的 FNV-1a 风格哈希，不依赖外部 crate。
    pub fn file_hash(content: &[u8]) -> String {
        let mut hash: u64 = 0xcbf29ce484222325;
        for &byte in content {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{:016x}", hash)
    }

    /// 扫描索引漂移
    ///
    /// 对比源文件目录与索引目录中文件的哈希差异。
    pub fn scan_index_drift(
        &self,
        source_dir: &str,
        index_dir: &str,
    ) -> Vec<IndexDrift> {
        let source_path = self.data_dir.join(source_dir);
        let index_path = self.data_dir.join(index_dir);

        if !source_path.exists() || !index_path.exists() {
            return Vec::new();
        }

        let mut drifts = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&source_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let file_name = path.file_name().unwrap_or_default();
                let index_file = index_path.join(file_name);

                if !index_file.exists() {
                    continue;
                }

                let source_content = std::fs::read(&path).unwrap_or_default();
                let index_content = std::fs::read(&index_file).unwrap_or_default();

                let source_hash = Self::file_hash(&source_content);
                let index_hash = Self::file_hash(&index_content);

                if source_hash != index_hash {
                    drifts.push(IndexDrift {
                        source_path: path,
                        stored_hash: index_hash,
                        current_hash: source_hash,
                    });
                }
            }
        }

        tracing::info!(
            drifts = drifts.len(),
            "索引漂移扫描完成"
        );

        drifts
    }

    /// 执行清理（仅处理已确认的项）
    ///
    /// **重要：此方法只标记已确认的项，不执行任何实际删除。**
    /// 实际删除需要用户通过 Tauri 命令确认后执行。
    pub fn prepare_cleanup_plan(&self, stale_items: &[StaleItem]) -> Vec<CleanupItem> {
        stale_items
            .iter()
            .map(|item| {
                let action = match item.item_type {
                    StaleType::Skill => CleanupAction::Remove,
                    StaleType::DocMismatch => CleanupAction::UpdateDoc,
                    StaleType::IndexDrift => CleanupAction::Reindex,
                };
                CleanupItem {
                    item: item.clone(),
                    action,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_file_hash_deterministic() {
        let content = b"hello world";
        let hash1 = EntropyManager::file_hash(content);
        let hash2 = EntropyManager::file_hash(content);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_file_hash_different_content() {
        let hash1 = EntropyManager::file_hash(b"content A");
        let hash2 = EntropyManager::file_hash(b"content B");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_scan_stale_files_empty_dir() {
        let dir = std::env::temp_dir().join("entropy_test_empty");
        let _ = fs::create_dir_all(&dir);
        let mgr = EntropyManager::new(dir.clone());
        let result = mgr.scan_stale_files("nonexistent");
        assert!(result.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_prepare_cleanup_plan() {
        let dir = std::env::temp_dir().join("entropy_test_plan");
        let _ = fs::create_dir_all(&dir);
        let mgr = EntropyManager::new(dir);

        let items = vec![StaleItem {
            path: PathBuf::from("/tmp/old_skill.md"),
            last_accessed_days: 120,
            item_type: StaleType::Skill,
        }];

        let plan = mgr.prepare_cleanup_plan(&items);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].action, CleanupAction::Remove);
    }
}
