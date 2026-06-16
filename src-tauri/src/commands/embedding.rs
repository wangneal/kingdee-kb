use tauri::State;

use crate::app_state::AppState;
use crate::services::embedding::{EmbeddingModelConfig, EmbeddingProvider, RemoteEmbeddingConfig};

/// 获取当前 Embedding 模型状态（已配置/未配置）。
#[tauri::command]
pub async fn get_model_status(state: State<'_, AppState>) -> Result<bool, String> {
    let emb = state.embedding.read().map_err(|e| e.to_string())?;
    Ok(emb.is_ready())
}

/// 获取 Embedding 模型配置。
#[tauri::command]
pub async fn get_embedding_model_config(
    state: State<'_, AppState>,
) -> Result<EmbeddingModelConfig, String> {
    let mm = state.model_manager.read().map_err(|e| e.to_string())?;
    Ok(mm.embedding_config())
}

/// 设置 Embedding 提供商配置（在线 API 或 Ollama）。
///
/// 所有参数均为 Option，provider 为 None 时清除配置。
#[tauri::command]
pub async fn set_embedding_model_config(
    state: State<'_, AppState>,
    provider: Option<EmbeddingProvider>,
    api_key: Option<String>,
    base_url: Option<String>,
    model_name: Option<String>,
) -> Result<bool, String> {
    let remote_config = provider.map(|provider| {
        let provider_info = EmbeddingProvider::all_providers()
            .into_iter()
            .find(|p| p.provider == provider);

        RemoteEmbeddingConfig {
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
        }
    });

    {
        let mut mm = state.model_manager.write().map_err(|e| e.to_string())?;
        mm.set_remote_config(remote_config.clone())?;
    }

    // 同步配置到 EmbeddingService
    let mut emb = state.embedding.write().map_err(|e| e.to_string())?;
    emb.set_remote_config(remote_config);

    Ok(true)
}
