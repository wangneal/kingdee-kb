//! LLM 供应商管理 — 多供应商配置 + 自动选择
//!
//! 支持配置多个 LLM 供应商，每个供应商可配置多个 API Key 和多个模型。
//! 系统根据任务类型自动选择：
//!   - 文本对话 → 用户选择的默认供应商 + 默认模型
//!   - 图像理解 → 自动选择支持多模态的模型
//!   - OCR → 独立的 OCR 配置

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const REMOTE_MODEL_CACHE_TTL: Duration = Duration::from_secs(300);

fn is_official_anthropic_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.eq_ignore_ascii_case("api.anthropic.com"))
        })
        .unwrap_or(false)
}

/// 构造 Anthropic Messages API 的完整 URL
///
/// base_url 可能是 `https://api.anthropic.com/v1` 或 `https://api.anthropic.com`，
/// 需要归一化避免拼接出 `/v1/v1/messages`。
/// 此函数去除尾部的 `/v1` 后重新拼接，保证结果一致。
pub fn anthropic_messages_url(base_url: &str) -> String {
    let normalized = base_url.trim_end_matches('/');
    let normalized = normalized.trim_end_matches("/v1");
    format!("{}/v1/messages", normalized)
}

pub fn with_anthropic_headers(
    request: reqwest::RequestBuilder,
    url: &str,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let request = request.header("x-api-key", api_key);
    let request = if is_official_anthropic_url(url) {
        request
    } else {
        request.header("Authorization", format!("Bearer {}", api_key))
    };

    request
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-dangerous-direct-browser-access", "true")
        .header("Content-Type", "application/json")
}

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

/// 供应商管理器
pub struct LLMProviderManager {
    /// LLM 供应商列表
    providers: Vec<LLMProviderConfig>,
    /// OCR 配置
    ocr_config: Option<OcrProviderConfig>,
    /// Provider 使用策略
    provider_policy: ProviderPolicyConfig,
    /// 配置文件路径
    config_path: PathBuf,
    /// HTTP 客户端
    client: reqwest::Client,
    /// 端点模型列表短期缓存
    remote_model_cache: HashMap<String, RemoteModelCacheEntry>,
}

#[derive(Debug, Clone)]
struct RemoteModelCacheEntry {
    fetched_at: Instant,
    models: Vec<String>,
}

// ─── 实现 ───

impl LLMProviderManager {
    /// 创建供应商管理器
    pub fn new(data_dir: &PathBuf) -> Self {
        let config_path = data_dir.join("llm_providers.json");
        let mut manager = Self {
            providers: Vec::new(),
            ocr_config: None,
            provider_policy: ProviderPolicyConfig::default(),
            config_path,
            client: reqwest::Client::new(),
            remote_model_cache: HashMap::new(),
        };
        manager.load();
        manager
    }

    /// 从文件加载配置
    fn load(&mut self) {
        if !self.config_path.exists() {
            return;
        }

        if let Ok(content) = std::fs::read_to_string(&self.config_path) {
            if let Ok(config) = serde_json::from_str::<ProviderConfigFile>(&content) {
                self.providers = config.providers.unwrap_or_default();
                self.ocr_config = config.ocr_config;
                self.provider_policy = config.provider_policy.unwrap_or_default();
            }
        }
    }

    /// 保存配置到文件
    fn save(&self) -> Result<(), String> {
        let config = ProviderConfigFile {
            providers: Some(self.providers.clone()),
            ocr_config: self.ocr_config.clone(),
            provider_policy: Some(self.provider_policy.clone()),
        };

        let content =
            serde_json::to_string_pretty(&config).map_err(|e| format!("序列化失败: {}", e))?;

        std::fs::write(&self.config_path, content).map_err(|e| format!("写入失败: {}", e))?;

        Ok(())
    }

    // ─── LLM 供应商 CRUD ───

    /// 获取所有 LLM 供应商
    pub fn list_providers(&self) -> &[LLMProviderConfig] {
        &self.providers
    }

    /// 获取运行态允许使用的 LLM 供应商
    pub fn list_runtime_providers(&self) -> Vec<LLMProviderConfig> {
        self.providers
            .iter()
            .filter(|provider| self.is_provider_allowed(&provider.id, None))
            .cloned()
            .collect()
    }

    /// 获取 Provider 策略
    pub fn get_provider_policy(&self) -> ProviderPolicyConfig {
        self.provider_policy.clone()
    }

    /// 保存 Provider 策略
    pub fn set_provider_policy(&mut self, policy: ProviderPolicyConfig) -> Result<(), String> {
        validate_provider_policy(&policy)?;
        self.provider_policy = policy;
        self.save()
    }

    /// 判断 provider/model 是否允许使用
    pub fn is_provider_allowed(&self, provider_id: &str, model_id: Option<&str>) -> bool {
        provider_policy_effect(&self.provider_policy, provider_id, model_id)
            == ProviderPolicyEffect::Allow
    }

    /// 强制校验 provider/model 是否允许使用
    pub fn assert_provider_allowed(
        &self,
        provider_id: &str,
        model_id: Option<&str>,
    ) -> Result<(), String> {
        if self.is_provider_allowed(provider_id, model_id) {
            Ok(())
        } else {
            let target = model_id
                .map(|model| format!("{}:{}", provider_id, model))
                .unwrap_or_else(|| provider_id.to_string());
            Err(format!("Provider Policy 已禁止使用 {}", target))
        }
    }

    /// 读取端点模型列表的短期缓存
    pub fn cached_remote_models(
        &self,
        protocol: &LLMProtocol,
        base_url: &str,
        api_key: &str,
    ) -> Option<Vec<String>> {
        let cache_key = remote_model_cache_key(protocol, base_url, api_key);
        self.remote_model_cache.get(&cache_key).and_then(|entry| {
            if entry.fetched_at.elapsed() <= REMOTE_MODEL_CACHE_TTL {
                Some(entry.models.clone())
            } else {
                None
            }
        })
    }

    /// 写入端点模型列表的短期缓存
    pub fn remember_remote_models(
        &mut self,
        protocol: &LLMProtocol,
        base_url: &str,
        api_key: &str,
        models: Vec<String>,
    ) {
        let cache_key = remote_model_cache_key(protocol, base_url, api_key);
        self.remote_model_cache.insert(
            cache_key,
            RemoteModelCacheEntry {
                fetched_at: Instant::now(),
                models,
            },
        );
    }

    /// 从端点的 /models 列表读取模型名称
    pub async fn fetch_remote_models_from_endpoint(
        protocol: &LLMProtocol,
        base_url: &str,
        api_key: &str,
    ) -> Result<Vec<String>, String> {
        let url = models_endpoint_url(base_url)?;
        if *protocol != LLMProtocol::Local && api_key.trim().is_empty() {
            return Err("请先填写 API Key，再获取端点模型列表".to_string());
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
        let request = client.get(&url);
        let request = match protocol {
            LLMProtocol::Anthropic => with_anthropic_headers(request, &url, api_key),
            LLMProtocol::OpenAI => request.header("Authorization", format!("Bearer {}", api_key)),
            LLMProtocol::Local => request,
        };

        let response = request
            .send()
            .await
            .map_err(|e| format!("请求模型列表失败: {}", e))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("模型列表接口返回 {}: {}", status, body));
        }

        parse_remote_model_names(&body)
    }

    /// 获取默认供应商
    pub fn get_default_provider(&self) -> Option<&LLMProviderConfig> {
        self.providers.iter().find(|p| p.is_default)
    }

    /// 获取运行态默认供应商
    pub fn get_default_runtime_provider(&self) -> Option<&LLMProviderConfig> {
        self.providers
            .iter()
            .find(|p| p.is_default && self.is_provider_allowed(&p.id, None))
            .or_else(|| {
                self.providers
                    .iter()
                    .find(|p| self.is_provider_allowed(&p.id, None))
            })
    }

    /// 根据 ID 获取供应商
    pub fn get_provider(&self, id: &str) -> Option<&LLMProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// 添加供应商
    pub fn add_provider(&mut self, provider: LLMProviderConfig) -> Result<(), String> {
        validate_provider_endpoint(&provider)?;
        // 检查 ID 唯一性
        if self.providers.iter().any(|p| p.id == provider.id) {
            return Err(format!("供应商 ID '{}' 已存在", provider.id));
        }

        // 如果是第一个供应商，设为默认
        let mut provider = provider;
        if self.providers.is_empty() {
            provider.is_default = true;
            // 如果有模型，设第一个为默认
            if let Some(first_model) = provider.models.first_mut() {
                first_model.is_default = true;
            }
            // 如果有 API Key，设第一个为默认
            if let Some(first_key) = provider.api_keys.first_mut() {
                first_key.is_default = true;
            }
        }

        self.providers.push(provider);
        self.save()
    }

    /// 更新供应商
    pub fn update_provider(&mut self, id: &str, provider: LLMProviderConfig) -> Result<(), String> {
        validate_provider_endpoint(&provider)?;
        let index = self
            .providers
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", id))?;

        self.providers[index] = provider;
        self.save()
    }

    /// 删除供应商
    pub fn delete_provider(&mut self, id: &str) -> Result<(), String> {
        let index = self
            .providers
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", id))?;

        let was_default = self.providers[index].is_default;
        self.providers.remove(index);

        // 如果删除的是默认供应商，将第一个设为默认
        if was_default && !self.providers.is_empty() {
            self.providers[0].is_default = true;
        }

        self.save()
    }

    /// 设置默认供应商
    pub fn set_default(&mut self, id: &str) -> Result<(), String> {
        for provider in &mut self.providers {
            provider.is_default = provider.id == id;
        }
        self.save()
    }

    // ─── API Key 管理 ───

    /// 添加 API Key 到供应商
    pub fn add_api_key(&mut self, provider_id: &str, api_key: ApiKeyConfig) -> Result<(), String> {
        let provider = self
            .providers
            .iter_mut()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?;

        // 如果是第一个 Key，设为默认
        if provider.api_keys.is_empty() {
            let mut api_key = api_key;
            api_key.is_default = true;
            provider.api_keys.push(api_key);
        } else {
            provider.api_keys.push(api_key);
        }

        self.save()
    }

    /// 更新 API Key
    pub fn update_api_key(
        &mut self,
        provider_id: &str,
        api_key: ApiKeyConfig,
    ) -> Result<(), String> {
        let provider = self
            .providers
            .iter_mut()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?;

        let index = provider
            .api_keys
            .iter()
            .position(|k| k.id == api_key.id)
            .ok_or_else(|| format!("API Key '{}' 不存在", api_key.id))?;

        provider.api_keys[index] = api_key;
        self.save()
    }

    /// 删除 API Key
    pub fn delete_api_key(&mut self, provider_id: &str, key_id: &str) -> Result<(), String> {
        let provider = self
            .providers
            .iter_mut()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?;

        let index = provider
            .api_keys
            .iter()
            .position(|k| k.id == key_id)
            .ok_or_else(|| format!("API Key '{}' 不存在", key_id))?;

        let was_default = provider.api_keys[index].is_default;
        provider.api_keys.remove(index);

        // 如果删除的是默认 Key，将第一个设为默认
        if was_default && !provider.api_keys.is_empty() {
            provider.api_keys[0].is_default = true;
        }

        self.save()
    }

    /// 设置默认 API Key
    pub fn set_default_api_key(&mut self, provider_id: &str, key_id: &str) -> Result<(), String> {
        let provider = self
            .providers
            .iter_mut()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?;

        for key in &mut provider.api_keys {
            key.is_default = key.id == key_id;
        }

        self.save()
    }

    // ─── 模型管理 ───

    /// 添加模型到供应商
    pub fn add_model(&mut self, provider_id: &str, model: ModelConfig) -> Result<(), String> {
        let provider = self
            .providers
            .iter_mut()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?;

        // 如果是第一个模型，设为默认
        if provider.models.is_empty() {
            let mut model = model;
            model.is_default = true;
            provider.models.push(model);
        } else {
            provider.models.push(model);
        }

        self.save()
    }

    /// 更新模型
    pub fn update_model(&mut self, provider_id: &str, model: ModelConfig) -> Result<(), String> {
        let provider = self
            .providers
            .iter_mut()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?;

        let index = provider
            .models
            .iter()
            .position(|m| m.id == model.id)
            .ok_or_else(|| format!("模型 '{}' 不存在", model.id))?;

        provider.models[index] = model;
        self.save()
    }

    /// 删除模型
    pub fn delete_model(&mut self, provider_id: &str, model_id: &str) -> Result<(), String> {
        let provider = self
            .providers
            .iter_mut()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?;

        let index = provider
            .models
            .iter()
            .position(|m| m.id == model_id)
            .ok_or_else(|| format!("模型 '{}' 不存在", model_id))?;

        let was_default = provider.models[index].is_default;
        provider.models.remove(index);

        // 如果删除的是默认模型，将第一个设为默认
        if was_default && !provider.models.is_empty() {
            provider.models[0].is_default = true;
        }

        self.save()
    }

    /// 设置默认模型
    pub fn set_default_model(&mut self, provider_id: &str, model_id: &str) -> Result<(), String> {
        let provider = self
            .providers
            .iter_mut()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", provider_id))?;

        for model in &mut provider.models {
            model.is_default = model.id == model_id;
        }

        self.save()
    }

    // ─── OCR 配置 ───

    /// 获取 OCR 配置
    pub fn get_ocr_config(&self) -> Option<&OcrProviderConfig> {
        self.ocr_config.as_ref()
    }

    /// 设置 OCR 配置
    pub fn set_ocr_config(&mut self, config: OcrProviderConfig) -> Result<(), String> {
        self.ocr_config = Some(config);
        self.save()
    }

    /// 清除 OCR 配置
    pub fn clear_ocr_config(&mut self) -> Result<(), String> {
        self.ocr_config = None;
        self.save()
    }

    // ─── 多模态探测 ───

    /// 探测指定模型是否支持多模态
    pub async fn probe_model_multimodal(
        &self,
        provider: &LLMProviderConfig,
        model_name: &str,
        api_key: &str,
    ) -> bool {
        if provider.protocol != LLMProtocol::Local && api_key.is_empty() {
            return false;
        }

        // 用 1x1 透明图片测试
        let test_img = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

        match provider.protocol {
            LLMProtocol::OpenAI => {
                let url = format!(
                    "{}/chat/completions",
                    provider.base_url.trim_end_matches('/')
                );
                let result = self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .json(&serde_json::json!({
                        "model": model_name,
                        "messages": [{"role": "user", "content": [
                            {"type": "text", "text": "test"},
                            {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", test_img)}}
                        ]}],
                        "max_tokens": 1
                    }))
                    .send()
                    .await;

                let mut is_success = false;
                match result {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            is_success = true;
                        } else {
                            let text = resp.text().await.unwrap_or_default();
                            tracing::warn!("OpenAI multimodal probe with Base64 failed for model {}. Status: {}, Response: {}", model_name, status, text);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("OpenAI multimodal probe with Base64 request failed for model {}. Error: {:?}", model_name, e);
                    }
                }

                if is_success {
                    return true;
                }

                // 2. 如果 Base64 探测失败，尝试公网图片 URL 探测 (fallback)
                let public_img_url = "https://tauri.app/img/logo-colored.png";
                tracing::info!(
                    "Attempting OpenAI multimodal probe with public URL for model {}",
                    model_name
                );
                let result_url = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .json(&serde_json::json!({
                        "model": model_name,
                        "messages": [{"role": "user", "content": [
                            {"type": "text", "text": "test"},
                            {"type": "image_url", "image_url": {"url": public_img_url}}
                        ]}],
                        "max_tokens": 1
                    }))
                    .send()
                    .await;

                match result_url {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            is_success = true;
                        } else {
                            let text = resp.text().await.unwrap_or_default();
                            tracing::warn!("OpenAI multimodal probe with public URL failed for model {}. Status: {}, Response: {}", model_name, status, text);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("OpenAI multimodal probe with public URL request failed for model {}. Error: {:?}", model_name, e);
                    }
                }

                if is_success {
                    return true;
                }

                // 3. 如果依然失败，尝试本地临时文件路径探测 (适用于可以访问本地路径的本地/内网部署模型)
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(test_img) {
                    let temp_path = std::env::temp_dir().join("kingdee_probe_temp.png");
                    if std::fs::write(&temp_path, bytes).is_ok() {
                        if let Ok(abs_path) = temp_path.canonicalize() {
                            let file_url = format!(
                                "file:///{}",
                                abs_path.to_string_lossy().replace('\\', "/")
                            );
                            tracing::info!("Attempting OpenAI multimodal probe with local file path for model {}: {}", model_name, file_url);
                            let result_local = self
                                .client
                                .post(&url)
                                .header("Authorization", format!("Bearer {}", api_key))
                                .json(&serde_json::json!({
                                    "model": model_name,
                                    "messages": [{"role": "user", "content": [
                                        {"type": "text", "text": "test"},
                                        {"type": "image_url", "image_url": {"url": file_url}}
                                    ]}],
                                    "max_tokens": 1
                                }))
                                .send()
                                .await;

                            let _ = std::fs::remove_file(&temp_path);

                            match result_local {
                                Ok(resp) => {
                                    let status = resp.status();
                                    if status.is_success() {
                                        is_success = true;
                                    } else {
                                        let text = resp.text().await.unwrap_or_default();
                                        tracing::warn!("OpenAI multimodal probe with local path failed for model {}. Status: {}, Response: {}", model_name, status, text);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("OpenAI multimodal probe with local path request failed for model {}. Error: {:?}", model_name, e);
                                }
                            }
                        }
                    }
                }

                is_success
            }
            LLMProtocol::Anthropic => {
                let url = anthropic_messages_url(&provider.base_url);
                let result = with_anthropic_headers(self.client.post(&url), &url, api_key)
                    .json(&serde_json::json!({
                        "model": model_name,
                        "messages": [
                            {
                                "role": "user",
                                "content": [
                                    {
                                        "type": "image",
                                        "source": {
                                            "type": "base64",
                                            "media_type": "image/png",
                                            "data": test_img
                                        }
                                    },
                                    {
                                        "type": "text",
                                        "text": "test"
                                    }
                                ]
                            }
                        ],
                        "max_tokens": 1
                    }))
                    .send()
                    .await;

                match result {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            true
                        } else {
                            let text = resp.text().await.unwrap_or_default();
                            tracing::warn!("Anthropic multimodal probe failed for model {}. Status: {}, Response: {}", model_name, status, text);
                            false
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Anthropic multimodal probe request failed for model {}. Error: {:?}",
                            model_name,
                            e
                        );
                        false
                    }
                }
            }
            LLMProtocol::Local => {
                // 1. 尝试 Ollama 原生 /api/chat 接口
                let ollama_url = format!("{}/api/chat", provider.base_url.trim_end_matches('/'));
                let result = self
                    .client
                    .post(&ollama_url)
                    .json(&serde_json::json!({
                        "model": model_name,
                        "messages": [{
                            "role": "user",
                            "content": "test",
                            "images": [test_img]
                        }],
                        "stream": false
                    }))
                    .send()
                    .await;

                let mut ollama_success = false;
                match result {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            ollama_success = true;
                        } else {
                            let text = resp.text().await.unwrap_or_default();
                            tracing::warn!("Local Ollama multimodal probe failed for model {}. Status: {}, Response: {}", model_name, status, text);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Local Ollama multimodal probe request failed for model {}. Error: {:?}", model_name, e);
                    }
                }

                ollama_success
            }
        }
    }

    /// 探测供应商的默认模型是否支持多模态
    pub async fn probe_multimodal(&self, provider: &LLMProviderConfig) -> bool {
        let api_key = provider.get_default_key_value();
        let model_name = provider.get_default_model_name();
        self.probe_model_multimodal(provider, &model_name, &api_key)
            .await
    }

    /// 批量探测所有供应商所有模型的多模态能力
    pub async fn probe_all(&mut self) -> Vec<(String, String, bool)> {
        let mut results = Vec::new();

        // 克隆供应商列表以避免借用冲突
        let providers: Vec<LLMProviderConfig> = self.providers.clone();

        for provider in &providers {
            let api_key = provider.get_default_key_value();
            for model in &provider.models {
                let is_multimodal = self
                    .probe_model_multimodal(provider, &model.name, &api_key)
                    .await;
                results.push((provider.id.clone(), model.id.clone(), is_multimodal));
            }
        }

        // 更新模型的多模态状态
        for (provider_id, model_id, is_multimodal) in &results {
            if let Some(provider) = self.providers.iter_mut().find(|p| &p.id == provider_id) {
                if let Some(model) = provider.models.iter_mut().find(|m| &m.id == model_id) {
                    model.is_multimodal = Some(*is_multimodal);
                    model.last_probe_at = Some(chrono::Utc::now().to_rfc3339());
                }
            }
        }

        let _ = self.save();
        results
    }

    // ─── 自动选择 ───

    /// 获取支持多模态的供应商和模型
    pub fn get_multimodal_model(&self) -> Option<(&LLMProviderConfig, &ModelConfig)> {
        // 优先返回默认供应商的默认模型（如果支持多模态）
        if let Some(default_provider) = self.get_default_runtime_provider() {
            if let Some(default_model) = default_provider.get_default_model() {
                if default_model.is_multimodal == Some(true)
                    && self.is_provider_allowed(&default_provider.id, Some(&default_model.id))
                {
                    return Some((default_provider, default_model));
                }
            }
        }

        // 否则返回任意支持多模态的模型
        for provider in &self.providers {
            if !self.is_provider_allowed(&provider.id, None) {
                continue;
            }
            for model in &provider.models {
                if model.is_multimodal == Some(true)
                    && self.is_provider_allowed(&provider.id, Some(&model.id))
                {
                    return Some((provider, model));
                }
            }
        }

        None
    }

    /// 获取所有多模态候选模型（按优先级排序，用于自动回退）
    /// 返回 (api_key, base_url, model_name, provider_id, model_id, protocol)
    ///
    /// 合并有序列表：tier1（已探测）+ tier2（builtin DB）+ tier3（未知），去重
    pub fn get_vision_candidates(
        &self,
    ) -> Vec<(String, String, String, String, String, LLMProtocol)> {
        let mut seen = std::collections::HashSet::new();
        let mut candidates = Vec::new();

        // 辅助闭包：添加候选并去重
        let add_candidate = |api_key: String,
                             base_url: String,
                             model_name: String,
                             provider_id: String,
                             model_id: String,
                             protocol: LLMProtocol,
                             seen: &mut std::collections::HashSet<(String, String)>,
                             candidates: &mut Vec<(
            String,
            String,
            String,
            String,
            String,
            LLMProtocol,
        )>| {
            let key = (provider_id.clone(), model_name.clone());
            if seen.insert(key) {
                candidates.push((
                    api_key,
                    base_url,
                    model_name,
                    provider_id,
                    model_id,
                    protocol,
                ));
            }
        };

        // Tier 1: is_multimodal == Some(true) 的已确认模型
        for provider in &self.providers {
            if !self.is_provider_allowed(&provider.id, None) {
                continue;
            }
            for model in &provider.models {
                if model.is_multimodal == Some(true)
                    && self.is_provider_allowed(&provider.id, Some(&model.id))
                {
                    add_candidate(
                        provider.get_default_key_value(),
                        provider.base_url.clone(),
                        model.name.clone(),
                        provider.id.clone(),
                        model.id.clone(),
                        provider.protocol.clone(),
                        &mut seen,
                        &mut candidates,
                    );
                }
            }
        }

        // Tier 2: is_multimodal != Some(false) 且内置 DB 标记 supports_vision=true
        for provider in &self.providers {
            if !self.is_provider_allowed(&provider.id, None) {
                continue;
            }
            for model in &provider.models {
                if !self.is_provider_allowed(&provider.id, Some(&model.id)) {
                    continue;
                }
                if model.is_multimodal != Some(false) {
                    if let Some(true) = super::model_metadata::builtin_supports_vision(&model.name)
                    {
                        add_candidate(
                            provider.get_default_key_value(),
                            provider.base_url.clone(),
                            model.name.clone(),
                            provider.id.clone(),
                            model.id.clone(),
                            provider.protocol.clone(),
                            &mut seen,
                            &mut candidates,
                        );
                    }
                }
            }
        }

        // Tier 3: is_multimodal != Some(false) 且内置 DB 未明确标记 supports_vision=false 的未知模型
        for provider in &self.providers {
            if !self.is_provider_allowed(&provider.id, None) {
                continue;
            }
            for model in &provider.models {
                if !self.is_provider_allowed(&provider.id, Some(&model.id)) {
                    continue;
                }
                if model.is_multimodal != Some(false) {
                    // 排除内置 DB 明确标记为不支持视觉的模型
                    match super::model_metadata::builtin_supports_vision(&model.name) {
                        Some(false) => continue, // 已知不支持视觉 → 跳过
                        _ => {} // Some(true) 已在 tier 2 处理并去重，None（未知）继续
                    }
                    add_candidate(
                        provider.get_default_key_value(),
                        provider.base_url.clone(),
                        model.name.clone(),
                        provider.id.clone(),
                        model.id.clone(),
                        provider.protocol.clone(),
                        &mut seen,
                        &mut candidates,
                    );
                }
            }
        }

        candidates
    }

    /// 获取供应商的 API 配置（用于 LLM 调用）
    /// 返回 (api_key, base_url, model_name)
    pub fn get_provider_config(&self, id: Option<&str>) -> Option<(String, String, String)> {
        let provider = if let Some(id) = id {
            self.get_provider(id)
                .filter(|provider| self.is_provider_allowed(&provider.id, None))
        } else {
            self.get_default_runtime_provider()
        };

        provider.map(|p| {
            let api_key = p.get_default_key_value();
            let model = p.get_default_model_name();
            (api_key, p.base_url.clone(), model)
        })
    }

    /// 自动路由：根据输入内容选择最佳模型
    /// 返回 (api_key, base_url, model_name, provider_id, model_id)
    pub fn auto_route(&self, has_images: bool) -> Option<(String, String, String, String, String)> {
        if has_images {
            // 有图片 → 优先选择多模态模型
            if let Some((provider, model)) = self.get_multimodal_model() {
                let api_key = provider.get_default_key_value();
                return Some((
                    api_key,
                    provider.base_url.clone(),
                    model.name.clone(),
                    provider.id.clone(),
                    model.id.clone(),
                ));
            }
            // 没有多模态模型 → 降级到默认模型
        }

        // 默认：使用默认供应商的默认模型
        let provider = self.get_default_runtime_provider()?;
        let model = provider.get_default_model()?;
        if !self.is_provider_allowed(&provider.id, Some(&model.id)) {
            return None;
        }
        let api_key = provider.get_default_key_value();

        Some((
            api_key,
            provider.base_url.clone(),
            model.name.clone(),
            provider.id.clone(),
            model.id.clone(),
        ))
    }

    /// 获取下一个可用的 API Key（故障切换）
    /// 当前 Key 失败时，尝试同一供应商的其他 Key
    pub fn get_next_api_key(
        &self,
        provider_id: &str,
        failed_key_id: &str,
    ) -> Option<(String, String)> {
        let provider = self.get_provider(provider_id)?;

        // 找到失败的 Key 索引
        let failed_index = provider
            .api_keys
            .iter()
            .position(|k| k.id == failed_key_id)?;

        // 尝试下一个 Key
        for (i, key) in provider.api_keys.iter().enumerate() {
            if i > failed_index && !key.key.is_empty() {
                return Some((key.id.clone(), key.key.clone()));
            }
        }

        // 如果后面没有可用的，从头开始尝试（跳过失败的）
        for key in &provider.api_keys {
            if key.id != failed_key_id && !key.key.is_empty() {
                return Some((key.id.clone(), key.key.clone()));
            }
        }

        None
    }

    /// 获取供应商的所有 API Key（用于故障切换）
    pub fn get_all_api_keys(&self, provider_id: &str) -> Vec<(String, String)> {
        let provider = match self.get_provider(provider_id) {
            Some(p) => p,
            None => return Vec::new(),
        };

        provider
            .api_keys
            .iter()
            .filter(|k| !k.key.is_empty())
            .map(|k| (k.id.clone(), k.key.clone()))
            .collect()
    }

    /// 标记 API Key 为不可用（临时禁用）
    pub fn mark_key_unavailable(&mut self, provider_id: &str, key_id: &str) {
        // 暂时不做持久化，只在内存中标记
        // 后续可以添加 key_status 字段到 ApiKeyConfig
        tracing::warn!("API Key {}:{} 标记为不可用", provider_id, key_id);
    }

    /// 获取所有可用模型列表（用于前端选择器）
    pub fn list_all_models(&self) -> Vec<AvailableModel> {
        let mut models = Vec::new();

        for provider in &self.providers {
            if !self.is_provider_allowed(&provider.id, None) {
                continue;
            }
            let api_key = provider.get_default_key_value();
            for model in &provider.models {
                if !self.is_provider_allowed(&provider.id, Some(&model.id)) {
                    continue;
                }
                models.push(AvailableModel {
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                    model_id: model.id.clone(),
                    model_name: model.name.clone(),
                    is_default: provider.is_default && model.is_default,
                    is_multimodal: model.is_multimodal,
                    api_key: api_key.clone(),
                    base_url: provider.base_url.clone(),
                });
            }
        }

        // 默认模型排第一
        models.sort_by(|a, b| {
            if a.is_default && !b.is_default {
                std::cmp::Ordering::Less
            } else if !a.is_default && b.is_default {
                std::cmp::Ordering::Greater
            } else {
                a.provider_name
                    .cmp(&b.provider_name)
                    .then(a.model_name.cmp(&b.model_name))
            }
        });

        models
    }
}

/// 配置文件结构
#[derive(Debug, Serialize, Deserialize)]
struct ProviderConfigFile {
    #[serde(default)]
    providers: Option<Vec<LLMProviderConfig>>,
    #[serde(default)]
    ocr_config: Option<OcrProviderConfig>,
    #[serde(default)]
    provider_policy: Option<ProviderPolicyConfig>,
}

fn validate_provider_policy(policy: &ProviderPolicyConfig) -> Result<(), String> {
    for rule in &policy.rules {
        if rule.action != "provider.use" {
            return Err(format!("不支持的 Provider Policy 动作: {}", rule.action));
        }
        let resource = rule.resource.trim();
        if resource.is_empty() {
            return Err("Provider Policy 资源不能为空".to_string());
        }
    }
    Ok(())
}

fn provider_policy_effect(
    policy: &ProviderPolicyConfig,
    provider_id: &str,
    model_id: Option<&str>,
) -> ProviderPolicyEffect {
    let exact_model = model_id.map(|model| format!("{}:{}", provider_id, model));
    let provider_wildcard = format!("{}:*", provider_id);

    for rule in policy.rules.iter().rev() {
        if rule.action != "provider.use" {
            continue;
        }
        let resource = rule.resource.trim();
        let matched = resource == "*"
            || resource == provider_id
            || resource == provider_wildcard
            || exact_model.as_deref() == Some(resource);
        if matched {
            return rule.effect.clone();
        }
    }

    policy.default_effect.clone()
}

fn validate_provider_endpoint(provider: &LLMProviderConfig) -> Result<(), String> {
    if provider.protocol == LLMProtocol::Local
        && provider.base_url.trim_end_matches('/').ends_with("/v1")
    {
        return Err("Local 协议仅支持 Ollama 原生根地址，Endpoint URL 不能以 /v1 结尾".to_string());
    }
    Ok(())
}

fn models_endpoint_url(base_url: &str) -> Result<String, String> {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return Err("请先填写 Endpoint URL".to_string());
    }
    let parsed =
        reqwest::Url::parse(normalized).map_err(|e| format!("Endpoint URL 无效: {}", e))?;
    if normalized.ends_with("/models") {
        return Ok(normalized.to_string());
    }
    let last_segment = parsed
        .path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).last());
    if last_segment.map(is_version_segment).unwrap_or(false) {
        Ok(format!("{}/models", normalized))
    } else {
        Ok(format!("{}/v1/models", normalized))
    }
}

fn is_version_segment(segment: &str) -> bool {
    let rest = match segment.strip_prefix('v') {
        Some(rest) => rest,
        None => return false,
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
}

fn remote_model_cache_key(protocol: &LLMProtocol, base_url: &str, api_key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    format!("{:?}", protocol).hash(&mut hasher);
    base_url.trim().trim_end_matches('/').hash(&mut hasher);
    api_key.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn parse_remote_model_names(body: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("解析模型列表失败: {}", e))?;
    let mut names = Vec::new();
    let mut seen = HashSet::new();

    let items = value
        .get("data")
        .and_then(|data| data.as_array())
        .or_else(|| value.as_array())
        .ok_or_else(|| "模型列表响应缺少 data 数组".to_string())?;

    for item in items {
        let name = item
            .as_str()
            .or_else(|| item.get("id").and_then(|id| id.as_str()))
            .or_else(|| item.get("name").and_then(|name| name.as_str()))
            .map(str::trim)
            .filter(|name| !name.is_empty());

        if let Some(name) = name {
            if seen.insert(name.to_string()) {
                names.push(name.to_string());
            }
        }
    }

    if names.is_empty() {
        return Err("模型列表响应中没有可用模型名称".to_string());
    }

    Ok(names)
}

// ─── 测试 ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_crud() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let mut manager = LLMProviderManager::new(&data_dir);

        // 添加
        let provider = LLMProviderConfig {
            id: "test1".to_string(),
            name: "Test Provider".to_string(),
            protocol: LLMProtocol::OpenAI,
            base_url: "https://api.openai.com/v1".to_string(),
            is_default: true,
            max_tokens: 4096,
            temperature: 0.3,
            api_keys: vec![ApiKeyConfig {
                id: "key1".to_string(),
                name: "\u{9ED8}\u{8BA4} Key".to_string(),
                key: "sk-test".to_string(),
                is_default: true,
            }],
            models: vec![ModelConfig {
                id: "model1".to_string(),
                name: "gpt-4o".to_string(),
                is_default: true,
                is_multimodal: None,
                last_probe_at: None,
                ..Default::default()
            }],
        };
        manager.add_provider(provider).unwrap();

        assert_eq!(manager.list_providers().len(), 1);
        assert!(manager.get_default_provider().is_some());

        // 更新
        let mut updated = manager.get_provider("test1").unwrap().clone();
        updated.name = "Updated".to_string();
        manager.update_provider("test1", updated).unwrap();
        assert_eq!(manager.get_provider("test1").unwrap().name, "Updated");

        // 删除
        manager.delete_provider("test1").unwrap();
        assert_eq!(manager.list_providers().len(), 0);
    }

    #[test]
    fn test_rejects_ambiguous_local_endpoint() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let mut manager = LLMProviderManager::new(&data_dir);
        let provider = LLMProviderConfig {
            id: "local1".to_string(),
            name: "Local Ollama".to_string(),
            protocol: LLMProtocol::Local,
            base_url: "http://localhost:11434/v1".to_string(),
            is_default: true,
            api_keys: vec![],
            models: vec![ModelConfig {
                id: "model1".to_string(),
                name: "qwen2.5:7b".to_string(),
                is_default: true,
                ..Default::default()
            }],
            max_tokens: 4096,
            temperature: 0.3,
        };

        assert!(manager.add_provider(provider).is_err());
    }

    #[test]
    fn test_rejects_removed_provider_fields() {
        let removed_shape = serde_json::json!({
            "id": "removed1",
            "name": "Removed Shape",
            "protocol": "openai",
            "base_url": "https://api.openai.com/v1",
            "is_default": true,
            "api_keys": [],
            "models": [],
            "max_tokens": 4096,
            "temperature": 0.3,
            "api_key": "removed"
        });

        assert!(serde_json::from_value::<LLMProviderConfig>(removed_shape).is_err());
    }

    #[test]
    fn test_models_endpoint_url_uses_versioned_base_url() {
        assert_eq!(
            models_endpoint_url("https://api.openai.com/v1").unwrap(),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            models_endpoint_url("https://dashscope.aliyuncs.com/compatible-mode/v1/").unwrap(),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/models"
        );
        assert_eq!(
            models_endpoint_url("https://example.com").unwrap(),
            "https://example.com/v1/models"
        );
    }

    #[test]
    fn test_parse_remote_model_names() {
        let body = serde_json::json!({
            "object": "list",
            "data": [
                { "id": "gpt-4o" },
                { "id": "gpt-4o" },
                { "name": "qwen-plus" },
                "deepseek-chat"
            ]
        })
        .to_string();

        assert_eq!(
            parse_remote_model_names(&body).unwrap(),
            vec![
                "gpt-4o".to_string(),
                "qwen-plus".to_string(),
                "deepseek-chat".to_string()
            ]
        );
    }

    #[test]
    fn test_multimodal_selection() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let mut manager = LLMProviderManager::new(&data_dir);

        // 添加两个供应商，一个支持多模态，一个不支持
        manager
            .add_provider(LLMProviderConfig {
                id: "text-only".to_string(),
                name: "Text Only".to_string(),
                protocol: LLMProtocol::OpenAI,
                base_url: "https://api.openai.com/v1".to_string(),
                is_default: true,
                max_tokens: 4096,
                temperature: 0.3,
                api_keys: vec![ApiKeyConfig {
                    id: "key1".to_string(),
                    name: "Key".to_string(),
                    key: "sk-key1".to_string(),
                    is_default: true,
                }],
                models: vec![ModelConfig {
                    id: "model1".to_string(),
                    name: "gpt-4".to_string(),
                    is_default: true,
                    is_multimodal: Some(false),
                    last_probe_at: None,
                    ..Default::default()
                }],
            })
            .unwrap();

        manager
            .add_provider(LLMProviderConfig {
                id: "multimodal".to_string(),
                name: "Multimodal".to_string(),
                protocol: LLMProtocol::OpenAI,
                base_url: "https://api.openai.com/v1".to_string(),
                is_default: false,
                max_tokens: 4096,
                temperature: 0.3,
                api_keys: vec![ApiKeyConfig {
                    id: "key2".to_string(),
                    name: "Key".to_string(),
                    key: "sk-key2".to_string(),
                    is_default: true,
                }],
                models: vec![ModelConfig {
                    id: "model2".to_string(),
                    name: "gpt-4o".to_string(),
                    is_default: true,
                    is_multimodal: Some(true),
                    last_probe_at: None,
                    ..Default::default()
                }],
            })
            .unwrap();

        // 自动选择应返回多模态供应商
        let (provider, model) = manager.get_multimodal_model().unwrap();
        assert_eq!(provider.id, "multimodal");
        assert_eq!(model.name, "gpt-4o");
    }
}
