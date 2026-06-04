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
pub struct IngestionQueue {
    items: Vec<QueueItem>,
    queue_path: PathBuf,
}

impl IngestionQueue {
    /// 创建队列，自动从磁盘加载已有状态
    pub fn new(data_dir: &Path) -> Self {
        let queue_path = data_dir.join("ingest-queue.json");
        let mut queue = Self {
            items: Vec::new(),
            queue_path,
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
            eprintln!("保存摄入队列失败: {}", e);
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
            eprintln!("保存摄入队列失败: {}", e);
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
            eprintln!("保存摄入队列失败: {}", e);
        }
        Some(result)
    }

    /// 将指定任务标记为已完成
    pub fn mark_done(&mut self, id: &str) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = "done".to_string();
            if let Err(e) = self.save() {
                eprintln!("保存摄入队列失败: {}", e);
            }
        }
    }

    /// 将指定任务标记为失败
    ///
    /// 自动递增重试次数。如果已达最大重试次数（3），
    /// 保留 failed 状态；否则可通过 retry_failed 重置。
    pub fn mark_failed(&mut self, id: &str, error: &str) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = "failed".to_string();
            item.retry_count += 1;
            item.error_message = Some(error.to_string());
            if let Err(e) = self.save() {
                eprintln!("保存摄入队列失败: {}", e);
            }
        }
    }

    /// 获取所有待处理任务
    pub fn list_pending(&self) -> Vec<&QueueItem> {
        self.items.iter().filter(|i| i.status == "pending").collect()
    }

    /// 将所有失败任务重置为待处理（手动重试）
    pub fn retry_failed(&mut self) {
        for item in self.items.iter_mut() {
            if item.status == "failed" && item.retry_count < 3 {
                item.status = "pending".to_string();
            }
        }
        if let Err(e) = self.save() {
            eprintln!("保存摄入队列失败: {}", e);
        }
    }

    pub fn retry_failed_for_project(&mut self, project_id: i64) {
        for item in self.items.iter_mut() {
            if item.project_id == project_id && item.status == "failed" && item.retry_count < 3 {
                item.status = "pending".to_string();
            }
        }
        if let Err(e) = self.save() {
            eprintln!("保存摄入队列失败: {}", e);
        }
    }

    /// 获取所有队列项
    pub fn all_items(&self) -> &[QueueItem] {
        &self.items
    }

    /// 将队列持久化到 JSON 文件（原子写入：临时文件 → rename）
    fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.items)
            .map_err(|e| format!("序列化摄入队列失败: {}", e))?;
        let tmp_path = self.queue_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json)
            .map_err(|e| format!("写入摄入队列临时文件失败: {}", e))?;
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
                eprintln!("解析摄入队列文件失败: {}", e);
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
            eprintln!("检测到上次异常退出，已将 processing 任务重置为 pending");
            if let Err(e) = self.save() {
                eprintln!("崩溃恢复后保存摄入队列失败: {}", e);
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

        let item = queue.dequeue_for_project(1).expect("项目一任务应存在");
        assert_eq!(item.id, project_a);
        queue.mark_failed(&project_a, "测试失败");
        queue.retry_failed_for_project(2);
        assert_eq!(
            queue
                .all_items()
                .iter()
                .find(|item| item.id == project_a)
                .expect("项目一任务应存在")
                .status,
            "failed"
        );
        assert_eq!(
            queue
                .dequeue_for_project(2)
                .expect("项目二任务应存在")
                .id,
            project_b
        );
    }
}
