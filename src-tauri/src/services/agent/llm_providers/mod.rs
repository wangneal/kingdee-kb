//! LLM 供应商管理 — 多供应商配置 + 自动选择
//!
//! 支持配置多个 LLM 供应商，每个供应商可配置多个 API Key 和多个模型。
//! 系统根据任务类型自动选择：
//!   - 文本对话 → 用户选择的默认供应商 + 默认模型
//!   - 图像理解 → 自动选择支持多模态的模型
//!   - OCR → 独立的 OCR 配置

mod anthropic;
pub mod probe;
mod seed;
mod types;

// re-export 公共类型，保持 `use crate::services::llm_providers::ApiKeyConfig` 等路径兼容
pub use types::*;
// re-export anthropic helpers，保持 `use crate::services::llm_providers::anthropic_messages_url` 路径兼容
pub use anthropic::{anthropic_messages_url, with_anthropic_headers};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use seed::{default_excluded_image_types, provider_policy_effect, remote_model_cache_key,
           validate_provider_policy, REMOTE_MODEL_CACHE_TTL};

/// 供应商管理器
pub struct LLMProviderManager {
    /// LLM 供应商列表
    providers: Vec<LLMProviderConfig>,
    /// OCR 配置
    ocr_config: Option<OcrProviderConfig>,
    /// 图片处理排除的类型（四分类 graph/text/table/image，默认排除 image 装饰图）
    excluded_image_types: Vec<String>,
    /// Provider 使用策略
    provider_policy: ProviderPolicyConfig,
    /// 配置文件路径
    config_path: PathBuf,
    /// HTTP 客户端（probe 函数重构为 free function 后此字段仅保留兼容性）
    #[allow(dead_code)]
    client: reqwest::Client,
    /// 端点模型列表短期缓存
    remote_model_cache: HashMap<String, RemoteModelCacheEntry>,
    /// 是否正在执行首次启动的 OpenCode Zen 默认 seed
    /// 为 true 时 save() 会拒绝（防止用户保存与 seed 写文件相互覆盖）
    seeding_in_progress: Arc<std::sync::atomic::AtomicBool>,
}

// ─── 实现 ───

impl LLMProviderManager {
    /// 同步创建供应商管理器
    ///
    /// **不**触发 seed —— 首次启动的 OpenCode Zen 默认配置 seed 由调用方显式调用
    /// [`seed_default_async`](Self::seed_default_async) 触发。分离原因是：
    /// 1. 同步构造可在无 tokio runtime 的环境（测试）正常工作
    /// 2. 异步 seed 由调用方在自己拥有 Arc 的上下文中 spawn，避免跨线程捕获裸 `&mut Self`
    pub fn new(data_dir: &PathBuf) -> Self {
        let config_path = data_dir.join("llm_providers.json");
        let mut manager = Self {
            providers: Vec::new(),
            ocr_config: None,
            excluded_image_types: default_excluded_image_types(),
            provider_policy: ProviderPolicyConfig::default(),
            config_path,
            client: reqwest::Client::new(),
            remote_model_cache: HashMap::new(),
            seeding_in_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
                // excluded_image_types 缺省回填默认（排除装饰图 image）
                self.excluded_image_types = config
                    .excluded_image_types
                    .unwrap_or_else(default_excluded_image_types);
            }
        }
    }

    /// 保存配置到文件
    fn save(&self) -> Result<(), String> {
        // 防竞态：首次启动 seed 进行中不允许 save()，避免覆盖用户输入
        // （seed 会在 fetch + 写文件完成后自动释放 flag）
        if self.seeding_in_progress.load(Ordering::SeqCst) {
            return Err("首次启动配置初始化中，请稍候再保存（约 8 秒）".to_string());
        }

        let config = ProviderConfigFile {
            providers: Some(self.providers.clone()),
            ocr_config: self.ocr_config.clone(),
            excluded_image_types: Some(self.excluded_image_types.clone()),
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
                fetched_at: std::time::Instant::now(),
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
        let url = seed::models_endpoint_url(base_url)?;
        if *protocol != LLMProtocol::Local && api_key.trim().is_empty() {
            return Err("请先填写 API Key，再获取端点模型列表".to_string());
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
        let request = client.get(&url);
        let request = match protocol {
            LLMProtocol::Anthropic => {
                anthropic::with_anthropic_headers(request, &url, api_key)
            }
            LLMProtocol::OpenAI => {
                request.header("Authorization", format!("Bearer {}", api_key))
            }
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

        seed::parse_remote_model_names(&body)
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

    /// 获取图片处理排除的类型（四分类 graph/text/table/image）
    pub fn get_excluded_image_types(&self) -> Vec<String> {
        self.excluded_image_types.clone()
    }

    /// 设置图片处理排除的类型，校验只允许四分类值
    pub fn set_excluded_image_types(&mut self, types: Vec<String>) -> Result<(), String> {
        for t in &types {
            if !matches!(t.as_str(), "graph" | "text" | "table" | "image") {
                return Err(format!("非法的图片类型: {}（仅允许 graph/text/table/image）", t));
            }
        }
        self.excluded_image_types = types;
        self.save()
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

// ─── 模块级 helpers（与 CRUD 同文件） ───

fn validate_provider_endpoint(provider: &LLMProviderConfig) -> Result<(), String> {
    if provider.protocol == LLMProtocol::Local
        && provider.base_url.trim_end_matches('/').ends_with("/v1")
    {
        return Err("Local 协议仅支持 Ollama 原生根地址，Endpoint URL 不能以 /v1 结尾".to_string());
    }
    Ok(())
}

// ─── 测试 ───

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;

    #[test]
    fn test_provider_crud() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let mut manager = LLMProviderManager::new(&data_dir);

        // 测试环境无 tokio runtime → seed 跳过，providers 初始为空
        assert_eq!(manager.list_providers().len(), 0);

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
            seed::models_endpoint_url("https://api.openai.com/v1").unwrap(),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            seed::models_endpoint_url("https://dashscope.aliyuncs.com/compatible-mode/v1/").unwrap(),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/models"
        );
        assert_eq!(
            seed::models_endpoint_url("https://example.com").unwrap(),
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
            seed::parse_remote_model_names(&body).unwrap(),
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

    /// 回归：seed_default_async 必须**同时**更新内存状态
    ///
    /// 修复前：seed 任务只写文件，`manager.providers` 永远是空，
    ///        Settings 页调用 list_providers() 看到空 Vec，
    ///        用户必须关 app 重启才能看到默认供应商
    /// 修复后：写文件 + 更新内存必须**同步**：要么都成功，要么都不动
    ///
    /// 端到端测试接受网络可达/不可达两种情况：
    /// - 网络可达：内存+文件都包含 opencode-zen
    /// - 网络不可达：内存+文件**都为空**（不再塞兜底模型，提示用户手动添加）
    /// 关键断言：内存状态与文件状态**完全一致**（不会被半更新撕裂）
    #[tokio::test]
    async fn seed_default_async_keeps_memory_and_file_in_sync() {
        let temp_dir = tempfile::tempdir().expect("创建临时目录失败");
        let data_dir = temp_dir.path().to_path_buf();
        // 关键：数据目录里没有 llm_providers.json → 触发 seed
        assert!(!data_dir.join("llm_providers.json").exists());

        let manager = LLMProviderManager::new(&data_dir);
        let arc_self = Arc::new(RwLock::new(manager));

        // seed 前：内存为空、文件不存在
        assert_eq!(
            arc_self.read().unwrap().list_providers().len(),
            0,
            "seed 前内存状态应为空"
        );
        assert!(
            !data_dir.join("llm_providers.json").exists(),
            "seed 前配置文件应不存在"
        );

        // 触发 seed
        LLMProviderManager::seed_default_async(&arc_self).await;

        // 关键不变量：内存与文件状态必须一致
        let memory_providers = arc_self.read().unwrap().list_providers().to_vec();
        let file_exists = data_dir.join("llm_providers.json").exists();

        assert_eq!(
            memory_providers.is_empty(),
            !file_exists,
            "内存与文件状态撕裂：内存 has {} 个供应商，文件存在 = {}。修复前只写文件、不更新内存。",
            memory_providers.len(),
            file_exists
        );

        if !memory_providers.is_empty() {
            // 网络可达分支：opencode-zen 必须存在且至少有一个模型
            assert_eq!(memory_providers[0].id, "opencode-zen", "默认供应商 id 应为 opencode-zen");
            assert!(
                !memory_providers[0].models.is_empty(),
                "默认供应商应至少包含一个模型"
            );
        }
        // 网络不可达分支：内存为空、文件未写入 — 这是允许的正确行为
        // （用户需要到 Settings 手动添加供应商）
    }

    /// 单元测试：seed_default_opencode_zen 是纯函数
    ///
    /// 验证：空模型列表 → 空供应商列表（不再"猜测"兜底模型）
    /// 验证：非空模型列表 → 单个 opencode-zen 供应商，第一个模型为默认
    #[test]
    fn seed_default_opencode_zen_returns_empty_when_no_models() {
        let result = seed::seed_default_opencode_zen(Vec::new());
        assert!(
            result.is_empty(),
            "空模型列表必须返回空供应商列表，禁止硬塞兜底模型"
        );
    }

    #[test]
    fn seed_default_opencode_zen_wraps_models_in_opencode_zen_provider() {
        let result = seed::seed_default_opencode_zen(vec!["gpt-free".to_string(), "claude-free".to_string()]);
        assert_eq!(result.len(), 1, "应只生成一个 opencode-zen 供应商");
        let provider = &result[0];
        assert_eq!(provider.id, "opencode-zen");
        assert!(provider.is_default);
        assert_eq!(provider.models.len(), 2);
        assert_eq!(provider.models[0].id, "gpt-free");
        assert!(provider.models[0].is_default, "第一个模型应为默认");
        assert!(!provider.models[1].is_default);
    }
}
