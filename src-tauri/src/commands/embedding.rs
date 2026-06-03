use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;

use crate::app_state::AppState;
use crate::services::embedding::{
    start_download_progress_polling, EmbeddingModelConfig, EmbeddingProvider, ProviderInfo,
    RemoteEmbeddingConfig,
};
use crate::services::metadata::KnowledgeStats;
use crate::services::vector_index::SearchResult;

/// 获取当前模型状态（就绪/未就绪）。
#[tauri::command]
pub async fn get_model_status(state: State<'_, AppState>) -> Result<bool, String> {
    let emb = state.embedding.read().map_err(|e| e.to_string())?;
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

    let model_result: Result<_, String> = {
        let mut mm = state.model_manager.write().map_err(|e| e.to_string())?;
        match mm.init() {
            Ok(()) => {
                stop_clone.store(true, Ordering::Relaxed);
                download_progress.store(100, Ordering::Relaxed);
                mm.take_model()
                    .ok_or_else(|| "Model initialized but no model returned".to_string())
            }
            Err(e) => {
                stop_clone.store(true, Ordering::Relaxed);
                download_progress.store(0, Ordering::Relaxed);
                Err(e)
            }
        }
    };

    match model_result {
        Ok(model) => {
            let mut emb = state.embedding.write().map_err(|e| e.to_string())?;
            emb.set_model(model);
            Ok(true)
        }
        Err(e) => Err(e),
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
    let mm = state.model_manager.read().map_err(|e| e.to_string())?;
    Ok(mm.embedding_config())
}

#[tauri::command]
pub async fn set_embedding_model_config(
    state: State<'_, AppState>,
    custom_model_dir: Option<String>,
    provider: Option<EmbeddingProvider>,
    api_key: Option<String>,
    base_url: Option<String>,
    model_name: Option<String>,
) -> Result<bool, String> {
    // 判断是否为远程提供商配置
    let is_remote = provider
        .as_ref()
        .is_some_and(|p| *p != EmbeddingProvider::Local);

    if is_remote {
        // 远程模式：配置在线 Embedding 提供商
        let provider = provider.unwrap();
        let provider_info = EmbeddingProvider::all_providers()
            .into_iter()
            .find(|p| p.provider == provider);

        let remote_config = RemoteEmbeddingConfig {
            provider: provider.clone(),
            api_key: api_key.unwrap_or_default(),
            base_url: base_url
                .or_else(|| {
                    provider_info
                        .as_ref()
                        .and_then(|p| p.default_base_url.clone())
                })
                .unwrap_or_default(),
            model_name: model_name
                .or_else(|| provider_info.as_ref().and_then(|p| p.default_model.clone()))
                .unwrap_or_default(),
        };

        {
            let mut mm = state.model_manager.write().map_err(|e| e.to_string())?;
            mm.set_remote_config(Some(remote_config.clone()))?;
        }

        // 同步配置到 EmbeddingService
        let mut emb = state.embedding.write().map_err(|e| e.to_string())?;
        emb.set_remote_config(Some(remote_config));
    } else {
        // 本地模式：配置本地 ONNX 模型
        let model = {
            let mut mm = state.model_manager.write().map_err(|e| e.to_string())?;
            mm.set_remote_config(None)?; // 清除远程配置
            mm.set_custom_model_dir(custom_model_dir)?;
            mm.init()?;
            mm.take_model()
                .ok_or("Model initialized but no model returned")?
        };
        let mut emb = state.embedding.write().map_err(|e| e.to_string())?;
        emb.set_model(model);
    }

    Ok(true)
}

#[tauri::command]
pub async fn embed_text(state: State<'_, AppState>, text: String) -> Result<Vec<f32>, String> {
    // 检查是否为远程模式
    let remote_config = {
        let emb = state.embedding.read().map_err(|e| e.to_string())?;
        emb.remote_config().cloned()
    };

    if let Some(config) = remote_config {
        // 远程模式：调用在线 Embedding API
        crate::services::embedding::remote_embed(&config, &text).await
    } else {
        // 本地模式：使用本地 ONNX 模型
        let mut emb = state.embedding.write().map_err(|e| e.to_string())?;
        emb.embed_text(&text)
    }
}

/// 批量嵌入多个文本
#[tauri::command]
pub async fn embed_batch(
    state: State<'_, AppState>,
    texts: Vec<String>,
) -> Result<Vec<Vec<f32>>, String> {
    // 检查是否为远程模式
    let remote_config = {
        let emb = state.embedding.read().map_err(|e| e.to_string())?;
        emb.remote_config().cloned()
    };

    if let Some(config) = remote_config {
        // 远程模式：调用在线 Embedding API
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        crate::services::embedding::remote_embed_batch(&config, &refs).await
    } else {
        // 本地模式：使用本地 ONNX 模型
        let mut emb = state.embedding.write().map_err(|e| e.to_string())?;
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        emb.embed_batch(&refs)
    }
}

/// 在 HNSW 索引中搜索相似向量
#[tauri::command]
pub async fn search_similar(
    state: State<'_, AppState>,
    query: Vec<f32>,
    top_k: u32,
) -> Result<Vec<SearchResult>, String> {
    let index = state.vector_index.read().map_err(|e| e.to_string())?;
    index.search(&query, top_k as usize)
}

/// 从磁盘加载向量索引
#[tauri::command]
pub async fn load_index(state: State<'_, AppState>) -> Result<usize, String> {
    let index = state.vector_index.read().map_err(|e| e.to_string())?;
    Ok(index.len())
}

/// 获取向量索引统计信息
#[tauri::command]
pub async fn get_index_stats(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let index = state.vector_index.read().map_err(|e| e.to_string())?;
    let stats = index.stats();
    serde_json::to_value(stats).map_err(|e| format!("Serialization error: {}", e))
}

/// 获取知识库统计信息（文档和分块数量）
#[tauri::command]
pub async fn get_knowledge_stats(
    state: State<'_, AppState>,
    project_id: Option<i64>,
) -> Result<KnowledgeStats, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_stats(project_id)
}

/// 获取所有可用的 Embedding 提供商列表
#[tauri::command]
pub async fn get_available_providers() -> Result<Vec<ProviderInfo>, String> {
    Ok(EmbeddingProvider::all_providers())
}
