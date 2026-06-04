//! 模型元数据分层获取
//!
//! 获取策略（按优先级）：
//!   1. 提供商原生 API（Anthropic / Google Gemini / Ollama）
//!   2. 内置模型数据库（model_specs.json）
//!   3. 保守默认值（context_window=4096, max_output_tokens=4096）
//!   4. 用户手动覆盖逐字段叠加（最高优先级，但不覆盖未设置的字段）

use super::llm_providers::{LLMProtocol, LLMProviderConfig};
use serde::{Deserialize, Serialize};

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
///
/// 获取策略（按优先级）：
///   1. 提供商原生 API（Anthropic / Google Gemini / Ollama）
///   2. 内置模型数据库（model_specs.json）
///   3. 保守默认值
///   4. 用户手动覆盖逐字段叠加（最高优先级，但不覆盖未设置的字段）
pub async fn resolve_metadata(provider: &LLMProviderConfig, model_name: &str) -> ModelMetadata {
    // 先从分层获取基础元数据（provider API → builtin DB → defaults）
    let mut meta = from_provider_api(provider, model_name)
        .await
        .or_else(|| from_builtin_db(model_name))
        .unwrap_or_default();

    // 用户覆盖：逐字段叠加（最高优先级）
    if let Some(model) = provider
        .models
        .iter()
        .find(|m| m.name == model_name || m.id == model_name)
    {
        if let Some(cw) = model.context_window {
            meta.context_window = cw;
        }
        if let Some(mo) = model.max_output_tokens {
            meta.max_output_tokens = mo;
        }
        if let Some(th) = model.supports_thinking {
            meta.supports_thinking = th;
        }
        // supports_vision: 仅在用户显式设置 is_multimodal 时覆盖
        // 否则保留从 provider API / builtin DB 获取的值
        if let Some(mm) = model.is_multimodal {
            meta.supports_vision = mm;
        }
    }

    meta
}

/// 同步查询内置数据库：模型是否支持视觉（用于候选筛选）
pub fn builtin_supports_vision(model_name: &str) -> Option<bool> {
    from_builtin_db(model_name).map(|m| m.supports_vision)
}

pub(crate) fn from_builtin_db(model_name: &str) -> Option<ModelMetadata> {
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

async fn from_provider_api(
    provider: &LLMProviderConfig,
    model_name: &str,
) -> Option<ModelMetadata> {
    let client = reqwest::Client::new();

    // Anthropic
    if provider.base_url.contains("anthropic.com") {
        let api_key = provider.api_keys.first()?.key.clone();
        let url = format!(
            "{}/v1/models/{}",
            provider.base_url.trim_end_matches('/'),
            model_name
        );
        let resp = client
            .get(&url)
            .header("x-api-key", &api_key)
            .send()
            .await
            .ok()?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await.ok()?;
            return Some(ModelMetadata {
                context_window: json["max_input_tokens"].as_u64()? as u32,
                max_output_tokens: json["max_tokens"].as_u64()? as u32,
                supports_thinking: json["capabilities"]["thinking"]["supported"]
                    .as_bool()
                    .unwrap_or(false),
                supports_vision: json["capabilities"]["image_input"]["supported"]
                    .as_bool()
                    .unwrap_or(false),
                supports_tools: true,
            });
        }
    }

    // Google Gemini
    if provider.base_url.contains("googleapis.com") {
        let api_key = provider.api_keys.first()?.key.clone();
        let url = format!(
            "{}/v1beta/models/{}?key={}",
            provider.base_url.trim_end_matches('/'),
            model_name,
            api_key
        );
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

    // Ollama 原生协议
    if provider.protocol == LLMProtocol::Local {
        let url = format!("{}/api/show", provider.base_url.trim_end_matches('/'));
        let resp = client
            .post(&url)
            .json(&serde_json::json!({"name": model_name}))
            .send()
            .await
            .ok()?;
        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await.ok()?;
            for (key, value) in json["model_info"].as_object()? {
                if key.ends_with(".context_length") {
                    return Some(ModelMetadata {
                        context_window: value.as_u64()? as u32,
                        max_output_tokens: 8192,
                        supports_thinking: false,
                        supports_vision: json["capabilities"].as_array().map_or(false, |caps| {
                            caps.iter().any(|c| c.as_str() == Some("vision"))
                        }),
                        supports_tools: true,
                    });
                }
            }
        }
    }

    None
}
