use tauri::State;

use crate::app_state::AppState;
use crate::services::ingestion::ingest_file as ingest_file_fn;
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

/// 处理所有待摄入队列任务。
#[tauri::command]
pub async fn process_ingestion_queue(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    process_pending_queue(&state)
}

/// 串行处理 pending 任务，供启动恢复和命令调用复用。
pub fn process_pending_queue(state: &AppState) -> Result<Vec<String>, String> {
    let mut processed = Vec::new();

    loop {
        let item = {
            let mut queue = state
                .ingest_queue
                .lock()
                .map_err(|e| format!("获取队列锁失败: {}", e))?;
            queue.dequeue()
        };

        let Some(item) = item else {
            break;
        };

        match process_one_queue_item(state, &item) {
            Ok(()) => {
                let mut queue = state
                    .ingest_queue
                    .lock()
                    .map_err(|e| format!("获取队列锁失败: {}", e))?;
                queue.mark_done(&item.id);
                processed.push(item.id);
            }
            Err(e) => {
                let mut queue = state
                    .ingest_queue
                    .lock()
                    .map_err(|err| format!("获取队列锁失败: {}", err))?;
                queue.mark_failed(&item.id, &e);
                tracing::warn!("摄入队列任务失败: id={}, error={}", item.id, e);
            }
        }
    }

    Ok(processed)
}

fn process_one_queue_item(state: &AppState, item: &QueueItem) -> Result<(), String> {
    state.ensure_embedding_ready();

    let raw_source = {
        let store = state
            .raw_sources
            .lock()
            .map_err(|e| format!("获取 raw_sources 锁失败: {}", e))?;
        store
            .find_by_identity(&item.project, &item.source_identity)?
            .ok_or_else(|| {
                format!(
                    "未找到原始资料: project={}, identity={}",
                    item.project, item.source_identity
                )
            })?
    };

    if raw_source.status != "active" {
        return Err(format!("原始资料已删除: {}", item.source_identity));
    }

    let result = ingest_file_fn(
        std::path::Path::new(&raw_source.storage_path),
        &item.project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        None,
        None,
        None,
    )?;

    let meta = state
        .metadata
        .lock()
        .map_err(|e| format!("获取 metadata 锁失败: {}", e))?;
    meta.update_document_raw_source_identity(result.document_id, &item.source_identity)?;
    Ok(())
}
