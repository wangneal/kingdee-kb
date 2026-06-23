//! 模型元数据查询
//!
//! 提供：
//!   - `builtin_supports_vision`：同步查询内置数据库（model_specs.json）判断模型是否支持视觉
//!   - `from_builtin_db`：同步查询内置数据库获取完整模型元数据
//!
//! 注意：原先的 `resolve_metadata`（异步 API 探测）和 `from_provider_api` 已删除，
//! 因为零调用者。上下文窗口大小由 `LLMProviderConfig::effective_context_window()` 同步获取。

use serde::{Deserialize, Serialize};

/// 模型元数据
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

/// 同步查询内置数据库：模型是否支持视觉（用于候选筛选）
pub fn builtin_supports_vision(model_name: &str) -> Option<bool> {
    from_builtin_db(model_name).map(|m| m.supports_vision)
}

/// 同步查询内置数据库：获取模型完整元数据
pub(crate) fn from_builtin_db(model_name: &str) -> Option<ModelMetadata> {
    let specs_str = include_str!("../../../resources/model_specs.json");
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
