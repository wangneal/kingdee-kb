//! 持久化摄入队列
//!
//! 基于 JSON 文件的持久化 FIFO 队列，支持崩溃恢复和重试机制。
//! 状态机：pending → processing → done / failed
//! 最大重试次数：3 次

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::path::PathBuf;

/// 单个摄入队列任务项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub project_id: i64,
    pub source_identity: String,
    pub status: String,
    pub retry_count: u32,
    pub error_message: Option<String>,
    pub created_at: String,
}

/// 持久化摄入队列
///
/// 双文件设计：
/// - `ingest-queue.json`：仅存储 pending/processing 的活跃任务（轻量，每次全量覆写）
/// - `ingest-queue-archive.jsonl`：append-only 的 JSONL 归档，存储已完成的 done/failed 任务
///
/// 状态机：pending → processing → done / failed
/// 最大重试次数：3 次
///
/// done/failed 任务从活跃文件移入归档，防止 JSON 文件无限膨胀导致每次读写延迟递增。

/// 持久化摄入队列
pub struct IngestionQueue {
    items: Vec<QueueItem>,
    queue_path: PathBuf,
    archive_path: PathBuf,
}

impl IngestionQueue {
    /// 创建队列，自动从磁盘加载活跃任务
    pub fn new(data_dir: &Path) -> Self {
        let queue_path = data_dir.join("ingest-queue.json");
        let archive_path = data_dir.join("ingest-queue-archive.jsonl");
        let mut queue = Self {
            items: Vec::new(),
            queue_path,
            archive_path,
        };
        queue.load();
        queue
    }

    /// 将任务加入队列末尾
    ///
    /// 返回生成的唯一任务 ID。
    pub fn enqueue(&mut self, project_id: i64, source_identity: &str) -> String {
        let id = format!("{}", chrono::Utc::now().timestamp_millis());
        let item = QueueItem {
            id: id.clone(),
            project_id,
            source_identity: source_identity.to_string(),
            status: "pending".to_string(),
            retry_count: 0,
            error_message: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.items.push(item);
        if let Err(e) = self.save() {
            tracing::error!("保存摄入队列失败: {}", e);
        }
        id
    }

    /// 取出下一个待处理任务（FIFO）
    ///
    /// 将状态从 pending 切换为 processing 后返回任务副本。
    pub fn dequeue(&mut self) -> Option<QueueItem> {
        let item = self.items.iter_mut().find(|i| i.status == "pending")?;
        item.status = "processing".to_string();
        let result = item.clone();
        if let Err(e) = self.save() {
            tracing::error!("保存摄入队列失败: {}", e);
        }
        Some(result)
    }

    pub fn dequeue_for_project(&mut self, project_id: i64) -> Option<QueueItem> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.status == "pending" && item.project_id == project_id)?;
        item.status = "processing".to_string();
        let result = item.clone();
        if let Err(e) = self.save() {
            tracing::error!("保存摄入队列失败: {}", e);
        }
        Some(result)
    }

    /// 将指定任务标记为已完成，移入归档并从活跃队列移除
    pub fn mark_done(&mut self, id: &str) {
        let pos = self.items.iter().position(|i| i.id == id);
        if let Some(idx) = pos {
            let mut item = self.items.swap_remove(idx);
            item.status = "done".to_string();
            if let Err(e) = self.append_to_archive(&item) {
                tracing::error!("归档完成任务失败: {}", e);
            }
            if let Err(e) = self.save() {
                tracing::error!("保存摄入队列失败: {}", e);
            }
        }
    }

    /// 将指定任务标记为失败，移入归档并从活跃队列移除
    ///
    /// 自动递增重试次数。如果已达最大重试次数（3），
    /// 保留 failed 状态；否则可通过 retry_failed 从归档恢复。
    pub fn mark_failed(&mut self, id: &str, error: &str) {
        let pos = self.items.iter().position(|i| i.id == id);
        if let Some(idx) = pos {
            let mut item = self.items.swap_remove(idx);
            item.status = "failed".to_string();
            item.retry_count += 1;
            item.error_message = Some(error.to_string());
            if let Err(e) = self.append_to_archive(&item) {
                tracing::error!("归档失败任务失败: {}", e);
            }
            if let Err(e) = self.save() {
                tracing::error!("保存摄入队列失败: {}", e);
            }
        }
    }

    /// 获取所有待处理任务
    pub fn list_pending(&self) -> Vec<&QueueItem> {
        self.items
            .iter()
            .filter(|i| i.status == "pending")
            .collect()
    }

    /// 从归档中恢复符合条件的所有失败任务到活跃队列
    ///
    /// `filter_project` 为 `None` 时不限项目，为 `Some(pid)` 时只恢复指定项目。
    fn revive_from_archive(&mut self, filter_project: Option<i64>) {
        for item in self.read_archive() {
            let project_match = filter_project.map_or(true, |pid| item.project_id == pid);
            if project_match && item.status == "failed" && item.retry_count < 3 {
                let mut revived = item.clone();
                revived.status = "pending".to_string();
                // 避免重复添加已在活跃队列中的项
                if !self.items.iter().any(|i| i.id == revived.id) {
                    self.items.push(revived);
                }
            }
        }
    }

    /// 将所有失败任务重置为待处理（从归档和活跃队列中查找）
    ///
    /// 从归档文件读取已归档的 failed 记录，恢复 pending 后加入活跃队列重新处理。
    pub fn retry_failed(&mut self) {
        // 从归档恢复所有项目的失败任务
        self.revive_from_archive(None);
        // 也处理仍在内存中（尚未归档）的 failed 项
        for item in self.items.iter_mut() {
            if item.status == "failed" && item.retry_count < 3 {
                item.status = "pending".to_string();
            }
        }
        if let Err(e) = self.save() {
            tracing::error!("保存摄入队列失败: {}", e);
        }
    }

    /// 将指定项目的所有失败任务重置为待处理
    pub fn retry_failed_for_project(&mut self, project_id: i64) {
        // 从归档恢复指定项目的失败任务
        self.revive_from_archive(Some(project_id));
        // 处理内存中的 failed 项
        for item in self.items.iter_mut() {
            if item.project_id == project_id && item.status == "failed" && item.retry_count < 3 {
                item.status = "pending".to_string();
            }
        }
        if let Err(e) = self.save() {
            tracing::error!("保存摄入队列失败: {}", e);
        }
    }

    /// 获取所有队列项
    pub fn all_items(&self) -> &[QueueItem] {
        &self.items
    }

    /// 获取前端可见队列项：活跃任务 + 未恢复的归档失败任务
    pub fn visible_items(&self) -> Vec<QueueItem> {
        let mut result = self.items.clone();
        let active_ids: std::collections::HashSet<String> =
            self.items.iter().map(|item| item.id.clone()).collect();
        let mut archived_failed: Vec<QueueItem> = self
            .read_archive()
            .into_iter()
            .filter(|item| item.status == "failed" && !active_ids.contains(&item.id))
            .collect();
        archived_failed.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result.extend(archived_failed.into_iter().take(200));
        result
    }

    /// 将一条记录追加到归档文件（JSONL 格式，append-only）
    fn append_to_archive(&self, item: &QueueItem) -> Result<(), String> {
        let line = serde_json::to_string(item).map_err(|e| format!("序列化归档项失败: {}", e))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.archive_path)
            .map_err(|e| format!("打开归档文件失败: {}", e))?;
        use std::io::Write;
        writeln!(file, "{}", line).map_err(|e| format!("写入归档文件失败: {}", e))?;
        file.flush()
            .map_err(|e| format!("刷入归档文件失败: {}", e))?;
        Ok(())
    }

    /// 读取归档文件中所有记录
    fn read_archive(&self) -> Vec<QueueItem> {
        let content = match std::fs::read_to_string(&self.archive_path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// 将队列持久化到 JSON 文件（原子写入：临时文件 → rename）
    ///
    /// 注意：`self.items` 中仅保留 pending/processing 的活跃任务；
    /// done/failed 任务在 mark_done/mark_failed 时已移入归档并从 self.items 移除。
    fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.items)
            .map_err(|e| format!("序列化摄入队列失败: {}", e))?;
        let tmp_path = self.queue_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json).map_err(|e| format!("写入摄入队列临时文件失败: {}", e))?;
        std::fs::rename(&tmp_path, &self.queue_path)
            .map_err(|e| format!("重命名摄入队列文件失败: {}", e))?;
        Ok(())
    }

    /// 从 JSON 文件加载队列
    ///
    /// 崩溃恢复：启动时将所有 processing 状态重置为 pending。
    fn load(&mut self) {
        let content = match std::fs::read_to_string(&self.queue_path) {
            Ok(c) => c,
            Err(_) => return,
        };
        self.items = match serde_json::from_str(&content) {
            Ok(items) => items,
            Err(e) => {
                tracing::error!("解析摄入队列文件失败: {}", e);
                return;
            }
        };
        // 崩溃恢复：processing → pending
        let mut changed = false;
        for item in self.items.iter_mut() {
            if item.status == "processing" {
                item.status = "pending".to_string();
                changed = true;
            }
        }
        if changed {
            tracing::info!("检测到上次异常退出，已将 processing 任务重置为 pending");
            if let Err(e) = self.save() {
                tracing::error!("崩溃恢复后保存摄入队列失败: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_queue_operations_do_not_affect_other_projects() {
        let dir = tempfile::tempdir().expect("创建临时目录失败");
        let mut queue = IngestionQueue::new(dir.path());
        let project_a = queue.enqueue(1, "a.md");
        let project_b = queue.enqueue(2, "b.md");

        // 取出项目一任务，标记失败 → 移入归档
        let item = queue.dequeue_for_project(1).expect("项目一任务应存在");
        assert_eq!(item.id, project_a);
        queue.mark_failed(&project_a, "测试失败");
        // mark_failed 后已从活跃队列移除，不应在 all_items 中
        assert!(
            queue.all_items().iter().all(|i| i.id != project_a),
            "失败任务应从活跃队列移除"
        );

        // 重试项目二的失败任务 → 不应影响项目一
        queue.retry_failed_for_project(2);
        // 项目一仍不在活跃队列中（不是项目二）
        assert!(
            queue.all_items().iter().all(|i| i.id != project_a),
            "跨项目重试不应恢复其他项目的失败项"
        );

        // 项目二的任务仍在
        assert_eq!(
            queue.dequeue_for_project(2).expect("项目二任务应存在").id,
            project_b
        );

        // 重试项目一的失败任务 → 从归档恢复
        queue.retry_failed_for_project(1);
        assert!(
            queue.all_items().iter().any(|i| i.id == project_a),
            "重试时应从归档恢复项目一的失败任务"
        );
        let restored = queue
            .all_items()
            .iter()
            .find(|i| i.id == project_a)
            .expect("项目一任务应已恢复");
        assert_eq!(restored.status, "pending");
    }
}
