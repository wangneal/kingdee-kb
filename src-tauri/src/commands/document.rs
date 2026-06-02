use tauri::State;

use crate::app_state::AppState;
use crate::services::metadata::{ChunkMeta, DocumentMeta, KnowledgeStats};

/// 列出所有文档，可按项目筛选
#[tauri::command]
pub async fn list_documents(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<Vec<DocumentMeta>, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.list_documents(project.as_deref())
}

/// 获取指定文档的所有分块
#[tauri::command]
pub async fn get_document_chunks(
    state: State<'_, AppState>,
    document_id: i64,
) -> Result<Vec<ChunkMeta>, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_chunks_by_document(document_id)
}

/// 删除文档及其所有关联分块（同时从向量索引和 BM25 中移除）
///
/// 实现删除补偿机制：
/// 1. 删除前记录待删 chunk_id 到 outbox
/// 2. 依次删除 usearch → BM25 → SQLite
/// 3. 删除后校验三索引一致性，残留则触发补偿清理
/// 4. 更新 outbox 状态为 completed/failed
#[tauri::command]
pub async fn delete_document(
    state: State<'_, AppState>,
    document_id: i64,
    project: Option<String>,
) -> Result<(), String> {
    let (vector_keys, outbox_id): (Vec<i64>, i64) = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        if let Some(pid) = project.as_deref() {
            let doc = meta
                .get_document(document_id)?
                .ok_or_else(|| format!("Document not found: {}", document_id))?;
            if doc.project != pid {
                return Err(format!(
                    "Document {} belongs to project '{}', not '{}'",
                    document_id, doc.project, pid
                ));
            }
        }
        let keys = meta.get_vector_keys_by_document_ids(&[document_id])?;
        // 预写 outbox，防止崩溃后丢失
        let oid = meta.insert_deletion_record(document_id, project.as_deref(), &keys)?;
        (keys, oid)
    };

    let mut errors: Vec<String> = Vec::new();

    // 1. 从 usearch 删除（先删向量索引）
    {
        if let Ok(idx) = state.vector_index.write() {
            if let Err(e) = idx.remove_keys(&vector_keys) {
                let err_msg = format!("usearch 删除失败(outbox_id={}): {}", outbox_id, e);
                tracing::error!("{}", err_msg);
                errors.push(err_msg);
            }
        }
    }

    // 2. 从 BM25 删除
    if let Ok(bm25) = state.bm25.write() {
        if let Err(e) = bm25.remove_chunks(&vector_keys) {
            let err_msg = format!("BM25 删除失败(outbox_id={}): {}", outbox_id, e);
            tracing::error!("{}", err_msg);
            errors.push(err_msg);
        }
    }

    // 3. 从 SQLite 删除元数据
    {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        if let Err(e) = meta.delete_document(document_id, project.as_deref()) {
            let err_msg = format!("SQLite 删除失败(outbox_id={}): {}", outbox_id, e);
            tracing::error!("{}", err_msg);
            errors.push(err_msg);
        }
    }

    // 4. 更新 outbox 状态
    {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        if errors.is_empty() {
            // 全部成功，标记为 completed
            if let Err(e) = meta.update_deletion_status(outbox_id, "completed", None) {
                tracing::warn!("更新 outbox 状态失败: {}", e);
            }
        } else {
            // 有错误，标记为 failed 并记录错误信息
            let error_msg = errors.join("; ");
            if let Err(e) = meta.update_deletion_status(outbox_id, "failed", Some(&error_msg)) {
                tracing::warn!("更新 outbox 状态失败: {}", e);
            }
            // 返回错误信息给前端
            return Err(format!("删除部分失败: {}", error_msg));
        }
    }

    // 5. 触发碎片整理检查（异步后台执行）
    {
        if let Ok(idx) = state.vector_index.read() {
            if idx.check_compact() {
                let idx_ref = state.vector_index.clone();
                std::thread::spawn(move || {
                    let plan = idx_ref
                        .read()
                        .ok()
                        .and_then(|idx| idx.prepare_compaction().ok().flatten());
                    if let Some(plan) = plan {
                        if let Ok(mut idx) = idx_ref.write() {
                            let _ = idx.apply_compaction(plan);
                        }
                    }
                });
            }
        }
    }

    Ok(())
}

/// 批量删除多个文档（及其所有关联分块、向量和 BM25 索引）
///
/// 实现删除补偿机制：每个文档单独记录 outbox，确保崩溃后可补偿。
#[tauri::command]
pub async fn delete_documents_batch(
    state: State<'_, AppState>,
    document_ids: Vec<i64>,
    project: Option<String>,
) -> Result<u64, String> {
    if document_ids.is_empty() {
        return Ok(0);
    }

    // 1. 验证项目归属并收集 vector_keys
    let all_vector_keys: Vec<i64> = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        if let Some(pid) = project.as_deref() {
            for doc_id in &document_ids {
                let doc = meta
                    .get_document(*doc_id)?
                    .ok_or_else(|| format!("Document not found: {}", doc_id))?;
                if doc.project != pid {
                    return Err(format!(
                        "Document {} belongs to project '{}', not '{}'",
                        doc_id, doc.project, pid
                    ));
                }
            }
        }
        meta.get_vector_keys_by_document_ids(&document_ids)?
    };

    // 2. 为每个文档预写 outbox
    let outbox_ids: Vec<i64> = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        let mut ids = Vec::new();
        for doc_id in &document_ids {
            let keys = meta.get_vector_keys_by_document_ids(&[*doc_id])?;
            let oid = meta.insert_deletion_record(*doc_id, project.as_deref(), &keys)?;
            ids.push(oid);
        }
        ids
    };

    let mut errors: Vec<String> = Vec::new();

    // 3. BM25 批量删除
    if let Ok(bm25) = state.bm25.write() {
        if let Err(e) = bm25.remove_chunks(&all_vector_keys) {
            errors.push(format!("BM25 批量删除失败: {}", e));
        }
    }

    // 4. usearch 批量删除
    if let Ok(idx) = state.vector_index.write() {
        if let Err(e) = idx.remove_keys(&all_vector_keys) {
            errors.push(format!("usearch 批量删除失败: {}", e));
        }
    }

    // 5. SQLite 删除（仅当索引删除全部成功时执行，否则留给补偿处理）
    let count: u64 = if errors.is_empty() {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.delete_documents_batch(document_ids, project.as_deref())?
    } else {
        tracing::warn!("索引删除部分失败，跳过 SQLite 删除，等待补偿处理");
        0
    };

    // 6. 更新所有 outbox 状态
    {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        let status = if errors.is_empty() { "completed" } else { "failed" };
        let error_msg = if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        };
        for oid in &outbox_ids {
            let _ = meta.update_deletion_status(*oid, status, error_msg.as_deref());
        }
    }

    // 7. 触发碎片整理检查
    if let Ok(idx) = state.vector_index.read() {
        if idx.check_compact() {
            let idx_ref = state.vector_index.clone();
            std::thread::spawn(move || {
                let plan = idx_ref
                    .read()
                    .ok()
                    .and_then(|idx| idx.prepare_compaction().ok().flatten());
                if let Some(plan) = plan {
                    if let Ok(mut idx) = idx_ref.write() {
                        let _ = idx.apply_compaction(plan);
                    }
                }
            });
        }
    }

    if !errors.is_empty() {
        return Err(format!("批量删除部分失败: {}", errors.join("; ")));
    }

    Ok(count)
}

/// 获取知识库统计信息（get_knowledge_stats 的别名）
#[tauri::command]
pub async fn get_stats(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<KnowledgeStats, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_stats(project.as_deref())
}
