//! LLM 供应商公共类型定义
//!
//! 包含供应商配置、模型配置、策略配置等核心数据结构。

use serde::{Deserialize, Serialize};
use std::time::Instant;

// ─── 类型定义 ───

/// API Key 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    /// 唯一标识
    pub id: String,
    /// 显示名称（如 "个人 Key"、"团队 Key"）
    #[serde(default)]
    pub name: String,
    /// API Key 值
    pub key: String,
    /// 是否为默认 Key
    #[serde(default)]
    pub is_default: bool,
}

/// 模型配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelConfig {
    /// 唯一标识
    pub id: String,
    /// 模型名称（如 "gpt-4o"、"claude-3-5-sonnet"）
    pub name: String,
    /// 是否为默认模型
    #[serde(default)]
    pub is_default: bool,
    /// 是否支持多模态（通过 API 探测）
    /// None = 未探测，Some(true) = 支持，Some(false) = 不支持
    #[serde(default)]
    pub is_multimodal: Option<bool>,
    /// 最后探测时间
    #[serde(default)]
    pub last_probe_at: Option<String>,
    // ─── P0-b 新增字段 ───
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub supports_thinking: Option<bool>,
}

/// LLM 供应商配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LLMProviderConfig {
    /// 唯一标识
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 协议类型
    pub protocol: LLMProtocol,
    /// Base URL
    pub base_url: String,
    /// 是否为默认供应商
    pub is_default: bool,
    /// API Key 列表
    pub api_keys: Vec<ApiKeyConfig>,
    /// 模型列表
    pub models: Vec<ModelConfig>,
    /// 最大上下文窗口（token 数，默认：4096）
    pub max_tokens: u32,
    /// 生成温度（默认：0.3）
    pub temperature: f32,
}

impl LLMProviderConfig {
    /// 检查是否已配置（有 API 密钥，或是本地模型）
    pub fn is_configured(&self) -> bool {
        if self.protocol == LLMProtocol::Local {
            return !self.base_url.is_empty();
        }
        self.api_keys.iter().any(|k| !k.key.is_empty())
    }

    /// 获取默认 API Key
    pub fn get_default_api_key(&self) -> Option<&ApiKeyConfig> {
        self.api_keys
            .iter()
            .find(|k| k.is_default)
            .or_else(|| self.api_keys.first())
    }

    /// 获取默认 API Key 值
    pub fn get_default_key_value(&self) -> String {
        self.get_default_api_key()
            .map(|key_config| key_config.key.clone())
            .unwrap_or_default()
    }

    /// 获取默认模型
    pub fn get_default_model(&self) -> Option<&ModelConfig> {
        self.models
            .iter()
            .find(|m| m.is_default)
            .or_else(|| self.models.first())
    }

    /// 获取默认模型名称
    pub fn get_default_model_name(&self) -> String {
        self.get_default_model()
            .map(|model_config| model_config.name.clone())
            .unwrap_or_default()
    }

    /// 获取默认模型配置的最大输出 token 数。
    /// 优先使用模型的 `max_output_tokens`，回退到 `max_tokens`，最终回退到 4096。
    pub fn effective_max_output_tokens(&self) -> u32 {
        if let Some(model) = self.get_default_model() {
            if let Some(mot) = model.max_output_tokens {
                if mot > 0 {
                    return mot;
                }
            }
        }
        // 回退：若模型未配置 max_output_tokens，使用 max_tokens（硬编码 4096）并 clamp 到安全范围
        self.max_tokens.clamp(4096, 32768)
    }

    /// 获取默认模型配置的上下文窗口大小。
    /// 优先使用模型的 `context_window`，回退到 `max_tokens`，最终回退到 4096。
    pub fn effective_context_window(&self) -> u32 {
        if let Some(model) = self.get_default_model() {
            if let Some(cw) = model.context_window {
                if cw > 0 {
                    return cw;
                }
            }
        }
        self.max_tokens.max(4096)
    }
}

/// LLM 协议类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LLMProtocol {
    /// OpenAI 兼容（GPT、通义千问、DeepSeek 等）
    OpenAI,
    /// Anthropic 兼容（Claude）
    Anthropic,
    /// 本地模型（Ollama 原生协议）
    Local,
}

/// Provider 策略效果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderPolicyEffect {
    Allow,
    Deny,
}

/// Provider 使用策略规则
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPolicyRule {
    /// 规则效果
    pub effect: ProviderPolicyEffect,
    /// 动作，目前固定为 provider.use
    pub action: String,
    /// 资源：*、provider_id、provider_id:* 或 provider_id:model_id
    pub resource: String,
}

/// Provider 策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPolicyConfig {
    /// 无匹配规则时的默认效果
    pub default_effect: ProviderPolicyEffect,
    /// 明确 allow/deny 规则
    pub rules: Vec<ProviderPolicyRule>,
}

impl Default for ProviderPolicyConfig {
    fn default() -> Self {
        Self {
            default_effect: ProviderPolicyEffect::Allow,
            rules: Vec::new(),
        }
    }
}

/// OCR 供应商配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrProviderConfig {
    /// 唯一标识
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 供应商类型
    pub provider: OcrProviderType,
    /// API Key
    pub api_key: String,
    /// Secret Key（百度需要）
    pub secret_key: Option<String>,
    /// 是否为默认 OCR
    pub is_default: bool,
}

/// OCR 供应商类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OcrProviderType {
    Baidu,
    Tencent,
    /// Mistral OCR（mistral-ocr-latest，表格/图表/版式强，单图 base64 支持）
    Mistral,
}

/// 可用模型（用于前端选择器）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableModel {
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub is_default: bool,
    pub is_multimodal: Option<bool>,
    pub api_key: String,
    pub base_url: String,
}

/// 配置文件结构
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProviderConfigFile {
    #[serde(default)]
    pub(crate) providers: Option<Vec<LLMProviderConfig>>,
    #[serde(default)]
    pub(crate) ocr_config: Option<OcrProviderConfig>,
    /// 图片处理排除的四分类类型（graph/text/table/image）
    #[serde(default)]
    pub(crate) excluded_image_types: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) provider_policy: Option<ProviderPolicyConfig>,
}

/// 端点模型列表短期缓存条目
#[derive(Debug, Clone)]
pub(crate) struct RemoteModelCacheEntry {
    pub(crate) fetched_at: Instant,
    pub(crate) models: Vec<String>,
}
