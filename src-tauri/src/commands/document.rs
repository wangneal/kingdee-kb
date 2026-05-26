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

/// 删除文档及其所有关联分块（同时从向量索引中移除向量）
#[tauri::command]
pub async fn delete_document(
    state: State<'_, AppState>,
    document_id: i64,
) -> Result<(), String> {
    let vector_keys: Vec<i64> = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.get_vector_keys_by_document_ids(&[document_id])?
    };

    {
        let idx = state.vector_index.lock().map_err(|e| e.to_string())?;
        for key in &vector_keys {
            let _ = idx.remove(*key as u64);
        }
    }

    {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.delete_document(document_id)?
    }

    Ok(())
}

/// 批量删除多个文档（及其所有关联分块和向量），在单个事务中执行
#[tauri::command]
pub async fn delete_documents_batch(
    state: State<'_, AppState>,
    document_ids: Vec<i64>,
) -> Result<u64, String> {
    if document_ids.is_empty() {
        return Ok(0);
    }

    let vector_keys: Vec<i64> = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.get_vector_keys_by_document_ids(&document_ids)?
    };

    {
        let idx = state.vector_index.lock().map_err(|e| e.to_string())?;
        for key in &vector_keys {
            let _ = idx.remove(*key as u64);
        }
    }

    let count: u64 = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.delete_documents_batch(document_ids)?
    };

    Ok(count)
}

/// 获取知识库统计信息（get_knowledge_stats 的别名）
#[tauri::command]
pub async fn get_stats(
    state: State<'_, AppState>,
) -> Result<KnowledgeStats, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_stats()
}
