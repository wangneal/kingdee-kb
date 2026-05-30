//! LLM 供应商管理 Tauri 命令

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::app_state::AppState;
use crate::services::llm_providers::{LLMProviderConfig, LLMProtocol, OcrProviderConfig, OcrProviderType};

// ─── LLM 供应商命令 ───

/// 获取所有 LLM 供应商
#[tauri::command]
pub async fn list_llm_providers(state: State<'_, AppState>) -> Result<Vec<LLMProviderConfig>, String> {
    let manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
    Ok(manager.list_providers().to_vec())
}

/// 添加 LLM 供应商
#[tauri::command]
pub async fn add_llm_provider(
    state: State<'_, AppState>,
    id: String,
    name: String,
    protocol: String,
    api_key: String,
    base_url: String,
    model: String,
) -> Result<(), String> {
    let protocol = match protocol.as_str() {
        "openai" => LLMProtocol::OpenAI,
        "anthropic" => LLMProtocol::Anthropic,
        "local" => LLMProtocol::Local,
        _ => return Err(format!("不支持的协议: {}", protocol)),
    };

    let provider = LLMProviderConfig {
        id,
        name,
        protocol,
        api_key,
        base_url,
        model,
        is_default: false,
        is_multimodal: None,
        last_probe_at: None,
    };

    let mut manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
    manager.add_provider(provider)
}

/// 更新 LLM 供应商
#[tauri::command]
pub async fn update_llm_provider(
    state: State<'_, AppState>,
    id: String,
    name: String,
    protocol: String,
    api_key: String,
    base_url: String,
    model: String,
) -> Result<(), String> {
    let protocol = match protocol.as_str() {
        "openai" => LLMProtocol::OpenAI,
        "anthropic" => LLMProtocol::Anthropic,
        "local" => LLMProtocol::Local,
        _ => return Err(format!("不支持的协议: {}", protocol)),
    };

    let provider = LLMProviderConfig {
        id: id.clone(),
        name,
        protocol,
        api_key,
        base_url,
        model,
        is_default: false,
        is_multimodal: None,
        last_probe_at: None,
    };

    let mut manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
    manager.update_provider(&id, provider)
}

/// 删除 LLM 供应商
#[tauri::command]
pub async fn delete_llm_provider(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
    manager.delete_provider(&id)
}

/// 设置默认 LLM 供应商
#[tauri::command]
pub async fn set_default_llm_provider(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
    manager.set_default(&id)
}

/// 探测单个供应商的多模态能力
#[tauri::command]
pub async fn probe_provider_multimodal(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    // 克隆供应商数据，避免在 await 时持有 MutexGuard
    let provider = {
        let manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
        manager.get_provider(&id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", id))?
            .clone()
    };

    // 创建临时管理器进行探测
    let temp_manager = crate::services::llm_providers::LLMProviderManager::new(&state.data_dir);
    Ok(temp_manager.probe_multimodal(&provider).await)
}

/// 批量探测所有供应商的多模态能力
#[tauri::command]
pub async fn probe_all_providers(state: State<'_, AppState>) -> Result<Vec<ProviderProbeResult>, String> {
    // 克隆供应商列表，避免在 await 时持有 MutexGuard
    let providers = {
        let manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
        manager.list_providers().to_vec()
    };

    // 创建临时管理器进行探测
    let temp_manager = crate::services::llm_providers::LLMProviderManager::new(&state.data_dir);

    // 手动探测每个供应商
    let mut results = Vec::new();
    for provider in &providers {
        let is_multimodal = temp_manager.probe_multimodal(provider).await;
        results.push(ProviderProbeResult {
            id: provider.id.clone(),
            is_multimodal,
        });
    }

    // 更新原始管理器中的探测结果
    {
        let mut manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
        for result in &results {
            if let Some(provider) = manager.list_providers().iter().find(|p| p.id == result.id) {
                let mut updated = provider.clone();
                updated.is_multimodal = Some(result.is_multimodal);
                updated.last_probe_at = Some(chrono::Utc::now().to_rfc3339());
                let _ = manager.update_provider(&result.id, updated);
            }
        }
    }

    Ok(results)
}

// ─── OCR 配置命令 ───

/// 获取 OCR 配置
#[tauri::command]
pub async fn get_ocr_config(state: State<'_, AppState>) -> Result<Option<OcrProviderConfig>, String> {
    let manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
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

    let mut manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
    manager.set_ocr_config(config)
}

/// 清除 OCR 配置
#[tauri::command]
pub async fn clear_ocr_config(state: State<'_, AppState>) -> Result<(), String> {
    let mut manager = state.llm_providers.lock().map_err(|e| e.to_string())?;
    manager.clear_ocr_config()
}

// ─── 响应类型 ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderProbeResult {
    pub id: String,
    pub is_multimodal: bool,
}
