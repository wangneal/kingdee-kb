use tauri::State;

use crate::app_state::AppState;
use crate::services::ingestion_queue::QueueItem;

/// 添加一个文件到摄入队列
#[tauri::command]
pub async fn enqueue_ingestion(
    state: State<'_, AppState>,
    project: String,
    source_identity: String,
) -> Result<String, String> {
    let mut queue = state
        .ingest_queue
        .lock()
        .map_err(|e| format!("获取队列锁失败: {}", e))?;
    Ok(queue.enqueue(&project, &source_identity))
}

/// 获取摄入队列中所有任务
#[tauri::command]
pub async fn list_ingestion_queue(
    state: State<'_, AppState>,
) -> Result<Vec<QueueItem>, String> {
    let queue = state
        .ingest_queue
        .lock()
        .map_err(|e| format!("获取队列锁失败: {}", e))?;
    Ok(queue.all_items().to_vec())
}

/// 重试所有失败的摄入任务
#[tauri::command]
pub async fn retry_failed_ingestions(state: State<'_, AppState>) -> Result<(), String> {
    let mut queue = state
        .ingest_queue
        .lock()
        .map_err(|e| format!("获取队列锁失败: {}", e))?;
    queue.retry_failed();
    Ok(())
}
