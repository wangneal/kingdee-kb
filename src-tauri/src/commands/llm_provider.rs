//! LLM 供应商管理 Tauri 命令

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::app_state::AppState;
use crate::services::llm_providers::{
    ApiKeyConfig, LLMProtocol, LLMProviderConfig, ModelConfig, OcrProviderConfig, OcrProviderType,
    ProviderPolicyConfig,
};

// ─── LLM 供应商命令 ───

/// 检查是否有已配置的 LLM 供应商
#[tauri::command]
pub async fn is_llm_configured(state: State<'_, AppState>) -> Result<bool, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
    Ok(manager
        .list_runtime_providers()
        .iter()
        .any(|p| p.is_configured()))
}

/// 获取所有 LLM 供应商
#[tauri::command]
pub async fn list_llm_providers(
    state: State<'_, AppState>,
) -> Result<Vec<LLMProviderConfig>, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
    Ok(manager.list_providers().to_vec())
}

/// 获取运行态允许使用的 LLM 供应商
#[tauri::command]
pub async fn list_runtime_llm_providers(
    state: State<'_, AppState>,
) -> Result<Vec<LLMProviderConfig>, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
    Ok(manager.list_runtime_providers())
}

/// 获取 Provider Policy
#[tauri::command]
pub async fn get_provider_policy(
    state: State<'_, AppState>,
) -> Result<ProviderPolicyConfig, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
    Ok(manager.get_provider_policy())
}

/// 保存 Provider Policy
#[tauri::command]
pub async fn set_provider_policy(
    state: State<'_, AppState>,
    policy: ProviderPolicyConfig,
) -> Result<ProviderPolicyConfig, String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.set_provider_policy(policy)?;
    sync_image_processor(&state, &manager);
    Ok(manager.get_provider_policy())
}

/// 获取端点可用模型列表（结果短期缓存在内存中）
#[tauri::command]
pub async fn fetch_llm_endpoint_models(
    state: State<'_, AppState>,
    protocol: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<RemoteModelListResult, String> {
    let protocol = match protocol.to_lowercase().as_str() {
        "openai" => LLMProtocol::OpenAI,
        "anthropic" => LLMProtocol::Anthropic,
        "local" => LLMProtocol::Local,
        _ => return Err(format!("不支持的协议: {}", protocol)),
    };
    let api_key = api_key.unwrap_or_default();

    {
        let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
        if let Some(models) = manager.cached_remote_models(&protocol, &base_url, &api_key) {
            return Ok(RemoteModelListResult {
                models,
                cached: true,
            });
        }
    }

    let models =
        crate::services::llm_providers::LLMProviderManager::fetch_remote_models_from_endpoint(
            &protocol, &base_url, &api_key,
        )
        .await?;

    {
        let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
        manager.remember_remote_models(&protocol, &base_url, &api_key, models.clone());
    }

    Ok(RemoteModelListResult {
        models,
        cached: false,
    })
}

/// 添加 LLM 供应商
#[tauri::command]
pub async fn add_llm_provider(
    state: State<'_, AppState>,
    id: String,
    name: String,
    protocol: String,
    base_url: String,
    api_keys: Vec<ApiKeyConfig>,
    models: Vec<ModelConfig>,
) -> Result<(), String> {
    let protocol = match protocol.to_lowercase().as_str() {
        "openai" => LLMProtocol::OpenAI,
        "anthropic" => LLMProtocol::Anthropic,
        "local" => LLMProtocol::Local,
        _ => return Err(format!("不支持的协议: {}", protocol)),
    };

    let provider = LLMProviderConfig {
        id,
        name,
        protocol,
        base_url,
        is_default: false,
        max_tokens: 4096,
        temperature: 0.3,
        api_keys,
        models,
    };

    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.add_provider(provider)?;

    // 同步 ImageProcessor 的 LLM 配置
    sync_image_processor(&state, &manager);

    Ok(())
}

/// 更新 LLM 供应商
#[tauri::command]
pub async fn update_llm_provider(
    state: State<'_, AppState>,
    id: String,
    name: String,
    protocol: String,
    base_url: String,
    api_keys: Vec<ApiKeyConfig>,
    models: Vec<ModelConfig>,
) -> Result<(), String> {
    let protocol = match protocol.to_lowercase().as_str() {
        "openai" => LLMProtocol::OpenAI,
        "anthropic" => LLMProtocol::Anthropic,
        "local" => LLMProtocol::Local,
        _ => return Err(format!("不支持的协议: {}", protocol)),
    };

    // 保留原有的 is_default 状态
    let is_default = {
        let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
        manager
            .get_provider(&id)
            .map(|p| p.is_default)
            .unwrap_or(false)
    };

    let provider = LLMProviderConfig {
        id: id.clone(),
        name,
        protocol,
        base_url,
        is_default,
        max_tokens: 4096,
        temperature: 0.3,
        api_keys,
        models,
    };

    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.update_provider(&id, provider)?;

    // 同步 ImageProcessor 的 LLM 配置
    sync_image_processor(&state, &manager);

    Ok(())
}

/// 删除 LLM 供应商
#[tauri::command]
pub async fn delete_llm_provider(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.delete_provider(&id)?;

    // 同步 ImageProcessor 的 LLM 配置
    sync_image_processor(&state, &manager);

    Ok(())
}

/// 设置默认 LLM 供应商
#[tauri::command]
pub async fn set_default_llm_provider(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.assert_provider_allowed(&id, None)?;
    manager.set_default(&id)?;

    // 同步 ImageProcessor 的 LLM 配置
    sync_image_processor(&state, &manager);

    Ok(())
}

// ─── API Key 管理命令 ───

/// 添加 API Key 到供应商
#[tauri::command]
pub async fn add_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    id: String,
    name: String,
    key: String,
) -> Result<(), String> {
    let api_key = ApiKeyConfig {
        id,
        name,
        key,
        is_default: false,
    };

    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.add_api_key(&provider_id, api_key)
}

/// 更新 API Key
#[tauri::command]
pub async fn update_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    id: String,
    name: String,
    key: String,
    is_default: bool,
) -> Result<(), String> {
    let api_key = ApiKeyConfig {
        id,
        name,
        key,
        is_default,
    };

    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.update_api_key(&provider_id, api_key)
}

/// 删除 API Key
#[tauri::command]
pub async fn delete_provider_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    key_id: String,
) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.delete_api_key(&provider_id, &key_id)
}

/// 设置默认 API Key
#[tauri::command]
pub async fn set_default_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    key_id: String,
) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.set_default_api_key(&provider_id, &key_id)
}

// ─── 模型管理命令 ───

/// 添加模型到供应商
#[tauri::command]
pub async fn add_model(
    state: State<'_, AppState>,
    provider_id: String,
    id: String,
    name: String,
) -> Result<(), String> {
    let model = ModelConfig {
        id,
        name,
        is_default: false,
        is_multimodal: None,
        last_probe_at: None,
        context_window: None,
        max_output_tokens: None,
        supports_thinking: None,
    };

    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.add_model(&provider_id, model)
}

/// 更新模型
#[tauri::command]
pub async fn update_model(
    state: State<'_, AppState>,
    provider_id: String,
    id: String,
    name: String,
    is_default: bool,
) -> Result<(), String> {
    // 保留原有的探测状态
    let (is_multimodal, last_probe_at) = {
        let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
        manager
            .get_provider(&provider_id)
            .and_then(|p| p.models.iter().find(|m| m.id == id))
            .map(|m| (m.is_multimodal, m.last_probe_at.clone()))
            .unwrap_or((None, None))
    };

    let model = ModelConfig {
        id,
        name,
        is_default,
        is_multimodal,
        last_probe_at,
        context_window: None,
        max_output_tokens: None,
        supports_thinking: None,
    };

    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.update_model(&provider_id, model)
}

/// 删除模型
#[tauri::command]
pub async fn delete_model(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.delete_model(&provider_id, &model_id)
}

/// 设置默认模型
#[tauri::command]
pub async fn set_default_model(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.set_default_model(&provider_id, &model_id)
}

// ─── 多模态探测命令 ───

/// 探测单个模型的多模态能力
#[tauri::command]
pub async fn probe_model_multimodal(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
) -> Result<bool, String> {
    // 克隆供应商数据，避免在 await 时持有 MutexGuard
    let (provider, model_name, api_key) = {
        let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
        let provider = manager
            .get_provider(&provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?
            .clone();
        let model = provider
            .models
            .iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| format!("模型 '{}' 不存在", model_id))?
            .clone();
        let api_key = provider.get_default_key_value();
        (provider, model.name, api_key)
    };

    // 创建临时管理器进行探测
    let temp_manager = crate::services::llm_providers::LLMProviderManager::new(&state.data_dir);
    let is_multimodal = temp_manager
        .probe_model_multimodal(&provider, &model_name, &api_key)
        .await;

    // 持久化探测结果（成功或失败都写回，避免重复探测）
    {
        let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
        if let Some(provider) = manager.get_provider(&provider_id).cloned() {
            let mut updated = provider;
            if let Some(model) = updated.models.iter_mut().find(|m| m.id == model_id) {
                model.is_multimodal = Some(is_multimodal);
                model.last_probe_at = Some(chrono::Utc::now().to_rfc3339());
            }
            let _ = manager.update_provider(&provider_id, updated);
        }
    }

    Ok(is_multimodal)
}

/// 探测单个供应商的多模态能力（使用默认模型）
#[tauri::command]
pub async fn probe_provider_multimodal(
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    // 克隆供应商数据，避免在 await 时持有 MutexGuard
    let provider = {
        let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
        manager
            .get_provider(&id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", id))?
            .clone()
    };

    // 创建临时管理器进行探测
    let temp_manager = crate::services::llm_providers::LLMProviderManager::new(&state.data_dir);
    Ok(temp_manager.probe_multimodal(&provider).await)
}

/// 批量探测所有供应商所有模型的多模态能力
#[tauri::command]
pub async fn probe_all_providers(
    state: State<'_, AppState>,
) -> Result<Vec<ModelProbeResult>, String> {
    // 克隆供应商列表，避免在 await 时持有 MutexGuard
    let providers = {
        let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
        manager.list_providers().to_vec()
    };

    // 创建临时管理器进行探测
    let temp_manager = crate::services::llm_providers::LLMProviderManager::new(&state.data_dir);

    // 手动探测每个供应商的每个模型
    let mut results = Vec::new();
    for provider in &providers {
        let api_key = provider.get_default_key_value();
        for model in &provider.models {
            let is_multimodal = temp_manager
                .probe_model_multimodal(provider, &model.name, &api_key)
                .await;
            results.push(ModelProbeResult {
                provider_id: provider.id.clone(),
                model_id: model.id.clone(),
                is_multimodal,
            });
        }
    }

    // 更新原始管理器中的探测结果
    {
        let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
        for result in &results {
            if let Some(provider) = manager
                .list_providers()
                .iter()
                .find(|p| p.id == result.provider_id)
            {
                let mut updated = provider.clone();
                if let Some(model) = updated.models.iter_mut().find(|m| m.id == result.model_id) {
                    model.is_multimodal = Some(result.is_multimodal);
                    model.last_probe_at = Some(chrono::Utc::now().to_rfc3339());
                }
                let _ = manager.update_provider(&result.provider_id, updated);
            }
        }
    }

    Ok(results)
}

// ─── OCR 配置命令 ───

/// 获取 OCR 配置
#[tauri::command]
pub async fn get_ocr_config(
    state: State<'_, AppState>,
) -> Result<Option<OcrProviderConfig>, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
    Ok(manager.get_ocr_config().cloned())
}

/// 保存 OCR 配置
#[tauri::command]
pub async fn save_ocr_config(
    state: State<'_, AppState>,
    id: String,
    name: String,
    provider: String,
    api_key: String,
    secret_key: Option<String>,
) -> Result<(), String> {
    let provider_type = match provider.as_str() {
        "baidu" => OcrProviderType::Baidu,
        "tencent" => OcrProviderType::Tencent,
        _ => return Err(format!("不支持的 OCR 供应商: {}", provider)),
    };

    let config = OcrProviderConfig {
        id,
        name,
        provider: provider_type,
        api_key,
        secret_key,
        is_default: true,
    };

    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.set_ocr_config(config)
}

/// 清除 OCR 配置
#[tauri::command]
pub async fn clear_ocr_config(state: State<'_, AppState>) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.clear_ocr_config()
}

// ─── 自动路由和模型列表 ───

/// 自动路由：根据输入内容选择最佳模型
#[tauri::command]
pub async fn auto_route_model(
    state: State<'_, AppState>,
    has_images: bool,
) -> Result<Option<AutoRouteResult>, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;

    match manager.auto_route(has_images) {
        Some((_api_key, base_url, model_name, provider_id, model_id)) => {
            Ok(Some(AutoRouteResult {
                provider_id,
                model_id,
                model_name,
                base_url,
                // 不返回 api_key 到前端（安全考虑）
            }))
        }
        None => Ok(None),
    }
}

/// 获取所有可用模型列表
#[tauri::command]
pub async fn list_available_models(
    state: State<'_, AppState>,
) -> Result<Vec<AvailableModelResult>, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
    let models = manager.list_all_models();

    Ok(models
        .into_iter()
        .map(|m| AvailableModelResult {
            provider_id: m.provider_id,
            provider_name: m.provider_name,
            model_id: m.model_id,
            model_name: m.model_name,
            is_default: m.is_default,
            is_multimodal: m.is_multimodal,
        })
        .collect())
}

/// 获取下一个可用的 API Key（故障切换）
#[tauri::command]
pub async fn get_next_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    failed_key_id: String,
) -> Result<Option<NextApiKeyResult>, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
    match manager.get_next_api_key(&provider_id, &failed_key_id) {
        Some((key_id, key_value)) => Ok(Some(NextApiKeyResult { key_id, key_value })),
        None => Ok(None),
    }
}

// ─── 辅助函数 ───

/// 同步 ImageProcessor 的 LLM 配置
fn sync_image_processor(
    state: &AppState,
    manager: &crate::services::llm_providers::LLMProviderManager,
) {
    if let Some(default) = manager.get_default_runtime_provider() {
        let api_key = default.get_default_key_value();
        let base_url = default.base_url.clone();
        let model = default.get_default_model_name();
        if let Ok(mut processor) = state.image_processor.write() {
            processor.update_llm_config(api_key, base_url, model);
        }
    }
}

// ─── 响应类型 ───

/// 探测结果（迁移期间预留）
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderProbeResult {
    pub id: String,
    pub is_multimodal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProbeResult {
    pub provider_id: String,
    pub model_id: String,
    pub is_multimodal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRouteResult {
    pub provider_id: String,
    pub model_id: String,
    pub model_name: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableModelResult {
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub is_default: bool,
    pub is_multimodal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextApiKeyResult {
    pub key_id: String,
    pub key_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteModelListResult {
    pub models: Vec<String>,
    pub cached: bool,
}
