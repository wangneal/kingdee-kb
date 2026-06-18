//! LLM 供应商管理 Tauri 命令

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::app_state::AppState;
use crate::error::AppError;
use crate::services::llm_providers::{
    ApiKeyConfig, LLMProtocol, LLMProviderConfig, ModelConfig, OcrProviderConfig, OcrProviderType,
    ProviderPolicyConfig,
};

/// 解析前端传入的协议字符串 → LLMProtocol 枚举
///
/// 支持大小写不敏感（前端可能传 "OpenAI"/"openai"/"OPENAI"）
fn parse_protocol_str(s: &str) -> Result<LLMProtocol, String> {
    match s.to_lowercase().as_str() {
        "openai" => Ok(LLMProtocol::OpenAI),
        "anthropic" => Ok(LLMProtocol::Anthropic),
        "local" => Ok(LLMProtocol::Local),
        _ => Err(format!("不支持的协议: {}", s)),
    }
}

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
    state.after_provider_change(&manager);
    Ok(manager.get_provider_policy())
}

/// 获取端点可用模型列表（结果短期缓存在内存中）
#[tauri::command]
pub async fn fetch_llm_endpoint_models(
    state: State<'_, AppState>,
    protocol: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<RemoteModelListResult, AppError> {
    let protocol = parse_protocol_str(&protocol)
        .map_err(AppError::InvalidArgument)?;
    let api_key = api_key.unwrap_or_default();

    {
        let manager = state
            .llm_providers
            .read()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if let Some(models) = manager.cached_remote_models(&protocol, &base_url, &api_key) {
            return Ok(RemoteModelListResult {
                models,
                cached: true,
            });
        }
    }

    // 服务层错误是 String；在 IPC 边界归类为 AppError
    let models =
        crate::services::llm_providers::LLMProviderManager::fetch_remote_models_from_endpoint(
            &protocol,
            &base_url,
            &api_key,
        )
        .await
        .map_err(|e| AppError::classify_llm_error("custom", &e))?;

    {
        let mut manager = state
            .llm_providers
            .write()
            .map_err(|e| AppError::Internal(e.to_string()))?;
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
    let protocol = parse_protocol_str(&protocol)?;

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

    state.after_provider_change(&manager);

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
    let protocol = parse_protocol_str(&protocol)?;

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

    state.after_provider_change(&manager);

    Ok(())
}

/// 删除 LLM 供应商
#[tauri::command]
pub async fn delete_llm_provider(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.delete_provider(&id)?;

    state.after_provider_change(&manager);

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

    state.after_provider_change(&manager);

    Ok(())
}

/// 设置默认 API Key
#[tauri::command]
pub async fn set_default_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    key_id: String,
) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.set_default_api_key(&provider_id, &key_id)?;
    // 默认 key 改了，ImageProcessor 必须跟着更新
    state.after_provider_change(&manager);
    Ok(())
}

// ─── 多模态探测命令 ───

/// 设置默认模型
#[tauri::command]
pub async fn set_default_model(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
) -> Result<(), String> {
    let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
    manager.set_default_model(&provider_id, &model_id)?;
    // 默认 model 改了，ImageProcessor 必须跟着更新
    state.after_provider_change(&manager);
    Ok(())
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

    // 直接构造 HTTP 客户端进行探测（不再创建临时 Manager）
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let is_multimodal = crate::services::agent::llm_providers::probe::probe_model_multimodal(
        &client,
        &provider,
        &model_name,
        &api_key,
    )
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

    // 直接构造 HTTP 客户端进行探测（不再创建临时 Manager）
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    // 手动探测每个供应商的每个模型
    let mut results = Vec::new();
    for provider in &providers {
        let api_key = provider.get_default_key_value();
        for model in &provider.models {
            let is_multimodal = crate::services::agent::llm_providers::probe::probe_model_multimodal(
                &client,
                provider,
                &model.name,
                &api_key,
            )
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
        "mistral" => OcrProviderType::Mistral,
        _ => return Err(format!("不支持的 OCR 供应商: {}", provider)),
    };

    let config = OcrProviderConfig {
        id,
        name,
        provider: provider_type.clone(),
        api_key: api_key.clone(),
        secret_key: secret_key.clone(),
        is_default: true,
    };

    // 写持久化存储（LLMProviderManager）
    {
        let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
        manager.set_ocr_config(config)?;
    }

    // 同步到运行时消费者（ImageProcessor）：业务侧 rig_agent/process_image
    // 读的是 ImageProcessor.ocr_config，不同步则 Settings 配的 OCR 凭证永不生效。
    let ocr_provider = crate::services::image_processor::OcrProvider::from_provider_type(&provider_type);
    let base_url = ocr_provider.default_base_url();
    if let Ok(mut processor) = state.image_processor.write() {
        processor.set_ocr_config(crate::services::image_processor::OcrConfig {
            provider: ocr_provider,
            api_key,
            secret_key,
            base_url,
        });
    }
    Ok(())
}

/// 清除 OCR 配置
#[tauri::command]
pub async fn clear_ocr_config(state: State<'_, AppState>) -> Result<(), String> {
    // 清持久化存储
    {
        let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
        manager.clear_ocr_config()?;
    }
    // 同步清除运行时消费者
    if let Ok(mut processor) = state.image_processor.write() {
        processor.clear_ocr_config();
    }
    Ok(())
}

/// 获取图片处理排除的类型（四分类 graph/text/table/image）
#[tauri::command]
pub async fn get_excluded_image_types(
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
    let types = manager.get_excluded_image_types();
    // 同步到运行时 ImageProcessor
    if let Ok(mut processor) = state.image_processor.write() {
        processor.set_excluded_image_types(types.clone());
    }
    Ok(types)
}

/// 设置图片处理排除的类型
#[tauri::command]
pub async fn set_excluded_image_types(
    state: State<'_, AppState>,
    types: Vec<String>,
) -> Result<(), String> {
    {
        let mut manager = state.llm_providers.write().map_err(|e| e.to_string())?;
        manager.set_excluded_image_types(types)?;
    }
    // 同步到运行时 ImageProcessor
    let types = {
        let manager = state.llm_providers.read().map_err(|e| e.to_string())?;
        manager.get_excluded_image_types()
    };
    if let Ok(mut processor) = state.image_processor.write() {
        processor.set_excluded_image_types(types);
    }
    Ok(())
}

// ─── 响应类型 ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProbeResult {
    pub provider_id: String,
    pub model_id: String,
    pub is_multimodal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteModelListResult {
    pub models: Vec<String>,
    pub cached: bool,
}
