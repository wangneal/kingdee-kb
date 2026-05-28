use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;

use crate::app_state::AppState;
use crate::services::embedding::{start_download_progress_polling, EmbeddingModelConfig};
use crate::services::metadata::KnowledgeStats;
use crate::services::vector_index::SearchResult;

/// 获取当前模型状态（就绪/未就绪）。
#[tauri::command]
pub async fn get_model_status(state: State<'_, AppState>) -> Result<bool, String> {
    let emb = state.embedding.lock().map_err(|e| e.to_string())?;
    Ok(emb.is_ready())
}

/// 初始化嵌入模型（首次调用时下载）。
#[tauri::command]
pub async fn init_model(state: State<'_, AppState>) -> Result<bool, String> {
    let download_progress = state.download_progress.clone();
    download_progress.store(0, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    start_download_progress_polling(
        &fastembed::EmbeddingModel::BGESmallZHV15,
        download_progress.clone(),
        stop,
    );

    let result = {
        let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
        mm.init()
    };

    stop_clone.store(true, Ordering::Relaxed);

    match result {
        Ok(()) => {
            download_progress.store(100, Ordering::Relaxed);
            let model = {
                let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
                mm.take_model()
                    .ok_or("Model initialized but no model returned")?
            };
            let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
            emb.set_model(model);
            Ok(true)
        }
        Err(e) => {
            download_progress.store(0, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// 获取嵌入模型的下载进度（0–100）。
#[tauri::command]
pub async fn get_download_progress(state: State<'_, AppState>) -> Result<u32, String> {
    Ok(state.download_progress.load(Ordering::Relaxed))
}

/// 嵌入单个文本 — 返回 512 维向量
#[tauri::command]
pub async fn get_embedding_model_config(
    state: State<'_, AppState>,
) -> Result<EmbeddingModelConfig, String> {
    let mm = state.model_manager.lock().map_err(|e| e.to_string())?;
    Ok(mm.embedding_config())
}

#[tauri::command]
pub async fn set_embedding_model_config(
    state: State<'_, AppState>,
    custom_model_dir: Option<String>,
) -> Result<bool, String> {
    {
        let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
        mm.set_custom_model_dir(custom_model_dir)?;
        mm.init()?;
    }

    let model = {
        let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
        mm.take_model()
            .ok_or("Model initialized but no model returned")?
    };
    let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
    emb.set_model(model);
    Ok(true)
}

#[tauri::command]
pub async fn embed_text(state: State<'_, AppState>, text: String) -> Result<Vec<f32>, String> {
    let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
    emb.embed_text(&text)
}

/// 批量嵌入多个文本
#[tauri::command]
pub async fn embed_batch(
    state: State<'_, AppState>,
    texts: Vec<String>,
) -> Result<Vec<Vec<f32>>, String> {
    let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    emb.embed_batch(&refs)
}

/// 在 HNSW 索引中搜索相似向量
#[tauri::command]
pub async fn search_similar(
    state: State<'_, AppState>,
    query: Vec<f32>,
    top_k: u32,
) -> Result<Vec<SearchResult>, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    index.search(&query, top_k as usize)
}

/// 从磁盘加载向量索引
#[tauri::command]
pub async fn load_index(state: State<'_, AppState>) -> Result<usize, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    Ok(index.len())
}

/// 获取向量索引统计信息
#[tauri::command]
pub async fn get_index_stats(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    let stats = index.stats();
    serde_json::to_value(stats).map_err(|e| format!("Serialization error: {}", e))
}

/// 获取知识库统计信息（文档和分块数量）
#[tauri::command]
pub async fn get_knowledge_stats(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<KnowledgeStats, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_stats(project.as_deref())
}
