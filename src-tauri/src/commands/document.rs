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

    // 1. 从 BM25 删除（记录错误但不阻塞）
    if let Ok(bm25) = state.bm25.lock() {
        if let Err(e) = bm25.remove_chunks(&vector_keys) {
            tracing::warn!("BM25 删除失败(outbox_id={}): {}", outbox_id, e);
        }
    }

    // 2. 从 usearch 删除
    if let Ok(idx) = state.vector_index.lock() {
        if let Err(e) = idx.remove_keys(&vector_keys) {
            tracing::warn!("usearch 删除失败(outbox_id={}): {}", outbox_id, e);
        }
    }

    // 3. 从 SQLite 删除
    {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.delete_document(document_id, project.as_deref())?;
    }

    // 4. outbox 标记完成
    if let Ok(meta) = state.metadata.lock() {
        let _ = meta.update_deletion_status(outbox_id, "completed", None);
    }

    // 5. 触发碎片整理检查（异步）
    if let Ok(idx) = state.vector_index.lock() {
        if idx.check_compact() {
            let idx_ref = state.vector_index.clone();
            std::thread::spawn(move || {
                if let Ok(mut idxx) = idx_ref.lock() {
                    let _ = idxx.compact();
                }
            });
        }
    }

    Ok(())
}

/// 批量删除多个文档（及其所有关联分块、向量和 BM25 索引）
#[tauri::command]
pub async fn delete_documents_batch(
    state: State<'_, AppState>,
    document_ids: Vec<i64>,
    project: Option<String>,
) -> Result<u64, String> {
    if document_ids.is_empty() {
        return Ok(0);
    }

    let vector_keys: Vec<i64> = {
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

    // 1. BM25 批量删除
    if let Ok(bm25) = state.bm25.lock() {
        let _ = bm25.remove_chunks(&vector_keys);
    }

    // 2. usearch 批量删除
    if let Ok(idx) = state.vector_index.lock() {
        let _ = idx.remove_keys(&vector_keys);
    }

    // 3. SQLite 删除
    let count: u64 = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.delete_documents_batch(document_ids, project.as_deref())?
    };

    // 4. 触发碎片整理检查
    if let Ok(idx) = state.vector_index.lock() {
        if idx.check_compact() {
            let idx_ref = state.vector_index.clone();
            std::thread::spawn(move || {
                if let Ok(mut idxx) = idx_ref.lock() {
                    let _ = idxx.compact();
                }
            });
        }
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
