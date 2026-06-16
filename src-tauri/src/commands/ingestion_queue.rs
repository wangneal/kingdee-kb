use tauri::State;

use crate::app_state::AppState;
use crate::services::ingestion::ingest_file as ingest_file_fn;
use crate::services::ingestion_pipeline::run_kb_compilation_flow;
use crate::services::ingestion_queue::QueueItem;

/// 添加一个文件到摄入队列
#[tauri::command]
pub async fn enqueue_ingestion(
    state: State<'_, AppState>,
    project_id: i64,
    source_identity: String,
) -> Result<String, String> {
    let mut queue = state
        .ingest_queue
        .lock()
        .map_err(|e| format!("获取队列锁失败: {}", e))?;
    Ok(queue.enqueue(project_id, &source_identity))
}

/// 获取摄入队列中所有任务
#[tauri::command]
pub async fn list_ingestion_queue(state: State<'_, AppState>) -> Result<Vec<QueueItem>, String> {
    let queue = state
        .ingest_queue
        .lock()
        .map_err(|e| format!("获取队列锁失败: {}", e))?;
    Ok(queue.visible_items())
}

#[tauri::command]
pub async fn retry_project_failed_ingestions(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<(), String> {
    let mut queue = state
        .ingest_queue
        .lock()
        .map_err(|e| format!("获取队列锁失败: {}", e))?;
    queue.retry_failed_for_project(project_id);
    Ok(())
}

#[tauri::command]
pub async fn process_project_ingestion_queue(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<String>, String> {
    process_pending_queue_for_project(&state, project_id)
}

/// 串行处理 pending 任务，供启动恢复和命令调用复用。
pub fn process_pending_queue(state: &AppState) -> Result<Vec<String>, String> {
    process_pending_queue_inner(state, None)
}

fn process_pending_queue_for_project(
    state: &AppState,
    project_id: i64,
) -> Result<Vec<String>, String> {
    process_pending_queue_inner(state, Some(project_id))
}

fn process_pending_queue_inner(
    state: &AppState,
    project_id: Option<i64>,
) -> Result<Vec<String>, String> {
    let mut processed = Vec::new();

    loop {
        let item = {
            let mut queue = state
                .ingest_queue
                .lock()
                .map_err(|e| format!("获取队列锁失败: {}", e))?;
            match project_id {
                Some(id) => queue.dequeue_for_project(id),
                None => queue.dequeue(),
            }
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
    state.ensure_bm25_ready();

    let raw_source = {
        let store = state
            .raw_sources
            .lock()
            .map_err(|e| format!("获取 raw_sources 锁失败: {}", e))?;
        store
            .find_by_identity(item.project_id, &item.source_identity)?
            .ok_or_else(|| {
                format!(
                    "未找到原始资料: project_id={}, identity={}",
                    item.project_id, item.source_identity
                )
            })?
    };

    if raw_source.status != "active" {
        return Err(format!("原始资料已删除: {}", item.source_identity));
    }

    // 防重复：已 `ingested` 的 raw_source 跳过 KB 编译（但仍允许底层 ingest 幂等更新）。
    // 用 raw_source.status 标记而非 SHA256，因为同一 source_identity 可能被
    // 编辑后再摄入，此时 SHA256 变了但 status 应仍反映"已处理过"。
    let already_ingested = raw_source.status == "ingested";

    let result = ingest_file_fn(
        std::path::Path::new(&raw_source.storage_path),
        item.project_id,
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
    drop(meta);

    // 触发 KB 编译：与主路径（commands::ingestion）行为对齐，让队列处理也生成 wiki 候选。
    if !already_ingested {
        // 读取已提取的文本（来自 `ingest_file_fn` 的 result.extracted_text）
        let text = result.extracted_text.as_deref().unwrap_or("");

        // 同步阻塞等待 KB 编译（与主路径行为一致）
        let source_identity = item.source_identity.clone();
        let sha256 = result.sha256.clone();
        let title = result.title.clone();
        let document_id = result.document_id;
        let project_id = item.project_id;

        let _ = tauri::async_runtime::block_on(async move {
            run_kb_compilation_flow(
                state,
                text,
                &source_identity,
                &sha256,
                project_id,
                &title,
                document_id,
                None, // 自动读取配置开关
                false,
            )
            .await
        });

        // 标记 raw_source 为 ingested（best-effort）
        if let Ok(store) = state.raw_sources.lock() {
            let _ = store.set_status(item.project_id, &item.source_identity, "ingested");
        }
    }

    Ok(())
}
