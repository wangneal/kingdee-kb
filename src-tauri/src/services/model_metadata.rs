//! 模型元数据分层获取
//! 
//! 获取策略（按优先级）：
//!   1. 用户手动覆盖（ModelConfig.context_window/max_output_tokens）
//!   2. 提供商原生 API（Anthropic / Google Gemini / Ollama）
//!   3. 内置模型数据库（model_specs.json）
//!   4. 保守默认值（context_window=4096, max_output_tokens=4096）

use serde::{Deserialize, Serialize};
use super::llm_providers::{LLMProtocol, LLMProviderConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub supports_thinking: bool,
    pub supports_vision: bool,
    pub supports_tools: bool,
}

impl Default for ModelMetadata {
    fn default() -> Self {
        Self {
            context_window: 4096,
            max_output_tokens: 4096,
            supports_thinking: false,
            supports_vision: false,
            supports_tools: false,
        }
    }
}

/// 分层获取模型元数据（异步，支持 API 探测）
pub async fn resolve_metadata(provider: &LLMProviderConfig, model_name: &str) -> ModelMetadata {
    // 优先级 1: 用户手动覆盖
    if let Some(model) = provider.models.iter().find(|m| m.name == model_name || m.id == model_name) {
        if model.context_window.is_some() || model.max_output_tokens.is_some() {
            return ModelMetadata {
                context_window: model.context_window.unwrap_or(4096),
                max_output_tokens: model.max_output_tokens.unwrap_or(4096),
                supports_thinking: model.supports_thinking.unwrap_or(false),
                supports_vision: model.is_multimodal.unwrap_or(false),
                supports_tools: true,
            };
        }
    }

    // 优先级 2: 提供商原生 API
    if let Some(meta) = from_provider_api(provider, model_name).await {
        return meta;
    }

    // 优先级 3: 内置模型数据库
    if let Some(meta) = from_builtin_db(model_name) {
        return meta;
    }

    // 优先级 4: 保守默认值
    tracing::warn!("Model '{}' not found in builtin DB, using conservative defaults", model_name);
    ModelMetadata::default()
}

fn from_builtin_db(model_name: &str) -> Option<ModelMetadata> {
    let specs_str = include_str!("../../resources/model_specs.json");
    let specs: serde_json::Value = serde_json::from_str(specs_str).ok()?;

    for (_provider, models) in specs.as_object()? {
        if let Some(spec) = models.get(model_name) {
            return Some(ModelMetadata {
                context_window: spec["context_window"].as_u64()? as u32,
                max_output_tokens: spec["max_output_tokens"].as_u64()? as u32,
                supports_thinking: spec["supports_thinking"].as_bool()?,
                supports_vision: spec["supports_vision"].as_bool()?,
                supports_tools: spec["supports_tools"].as_bool()?,
            });
        }
    }
    None
}

async fn from_provider_api(provider: &LLMProviderConfig, model_name: &str) -> Option<ModelMetadata> {
    let client = reqwest::Client::new();

    // Anthropic
    if provider.base_url.contains("anthropic.com") {
        let api_key = provider.api_keys.first()?.key.clone();
        let url = format!("{}/v1/models/{}", provider.base_url.trim_end_matches('/'), model_name);
        let resp = client.get(&url).header("x-api-key", &api_key).send().await.ok()?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await.ok()?;
            return Some(ModelMetadata {
                context_window: json["max_input_tokens"].as_u64()? as u32,
                max_output_tokens: json["max_tokens"].as_u64()? as u32,
                supports_thinking: json["capabilities"]["thinking"]["supported"].as_bool().unwrap_or(false),
                supports_vision: json["capabilities"]["image_input"]["supported"].as_bool().unwrap_or(false),
                supports_tools: true,
            });
        }
    }

    // Google Gemini
    if provider.base_url.contains("googleapis.com") {
        let api_key = provider.api_keys.first()?.key.clone();
        let url = format!("{}/v1beta/models/{}?key={}", provider.base_url.trim_end_matches('/'), model_name, api_key);
        let resp = client.get(&url).send().await.ok()?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await.ok()?;
            return Some(ModelMetadata {
                context_window: json["inputTokenLimit"].as_u64()? as u32,
                max_output_tokens: json["outputTokenLimit"].as_u64()? as u32,
                supports_thinking: json["thinking"].as_bool().unwrap_or(false),
                supports_vision: true,
                supports_tools: true,
            });
        }
    }

    // Ollama (Local)
    if provider.protocol == LLMProtocol::Local {
        let url = format!("{}/api/show", provider.base_url.trim_end_matches('/'));
        let resp = client.post(&url).json(&serde_json::json!({"name": model_name})).send().await.ok()?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await.ok()?;
            for (key, value) in json["model_info"].as_object()? {
                if key.ends_with(".context_length") {
                    return Some(ModelMetadata {
                        context_window: value.as_u64()? as u32,
                        max_output_tokens: 8192,
                        supports_thinking: false,
                        supports_vision: json["capabilities"].as_array()
                            .map_or(false, |caps| caps.iter().any(|c| c.as_str() == Some("vision"))),
                        supports_tools: true,
                    });
                }
            }
        }
    }

    None
}
