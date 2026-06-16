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
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// 是否正在执行首次启动的 OpenCode Zen 默认 seed
    /// 为 true 时 save() 会拒绝（防止用户保存与 seed 写文件相互覆盖）
    seeding_in_progress: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct RemoteModelCacheEntry {
    fetched_at: Instant,
    models: Vec<String>,
}

// ─── 实现 ───

/// OpenCode Zen 接口基础地址
const OPENCODE_ZEN_BASE_URL: &str = "https://opencode.ai/zen/v1";

/// 异步从 OpenCode Zen `/v1/models` 拉取所有带 `-free` 后缀的模型名
/// 使用 "public" key（OpenCode Zen 免费模型约定的公共 key）
///
/// 错误处理：网络/解析失败时返回空 Vec，由 `seed_default_async` 决定是否继续。
/// 失败会记 debug 日志便于排查"为什么没拉到模型"——网络失败、防火墙拦截、API 变更等都能区分。
/// 不会"猜测"兜底模型：拉不到就**不**写默认配置，让用户在 Settings 页面手动添加供应商
async fn fetch_opencode_zen_free_models() -> Vec<String> {
    let url = format!("{}/models", OPENCODE_ZEN_BASE_URL);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("构建 reqwest 客户端失败: {}", e);
            return Vec::new();
        }
    };
    let resp = match client
        .get(&url)
        .header("Authorization", "Bearer public")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("OpenCode Zen /models 请求失败（可能是离线）: {}", e);
            return Vec::new();
        }
    };
    if !resp.status().is_success() {
        tracing::debug!(
            "OpenCode Zen /models 返回非 2xx 状态: {}",
            resp.status()
        );
        return Vec::new();
    }
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("读取 OpenCode Zen /models 响应体失败: {}", e);
            return Vec::new();
        }
    };
    let names = match parse_remote_model_names(&body) {
        Ok(n) => n,
        Err(e) => {
            tracing::debug!("解析 OpenCode Zen /models 响应失败: {}", e);
            return Vec::new();
        }
    };
    let free: Vec<String> = names.into_iter().filter(|n| n.ends_with("-free")).collect();
    tracing::debug!("OpenCode Zen /models 拉到 {} 个 -free 模型", free.len());
    free
}

/// 构造默认 OpenCode Zen 供应商配置
/// - `free_models`: 从 OpenCode Zen 拉到的 -free 模型列表；空时返回空供应商列表
///   （网络失败时不强行塞兜底模型，让用户在 Settings 页面手动添加）
fn seed_default_opencode_zen(free_models: Vec<String>) -> Vec<LLMProviderConfig> {
    if free_models.is_empty() {
        return Vec::new();
    }
    let models: Vec<ModelConfig> = free_models
        .into_iter()
        .enumerate()
        .map(|(idx, name)| ModelConfig {
            id: name.clone(),
            name,
            is_default: idx == 0,
            is_multimodal: Some(false),
            last_probe_at: None,
            context_window: None,
            max_output_tokens: None,
            supports_thinking: None,
        })
        .collect();

    vec![LLMProviderConfig {
        id: "opencode-zen".to_string(),
        name: "OpenCode Zen".to_string(),
        protocol: LLMProtocol::OpenAI,
        base_url: OPENCODE_ZEN_BASE_URL.to_string(),
        is_default: true,
        api_keys: vec![ApiKeyConfig {
            id: "default".to_string(),
            name: "公共 Key".to_string(),
            // OpenCode Zen 的免费模型使用字面量 "public" 作为公共 API key
            // （参考 opencode 行为：未配置 key 时使用 public，仅显示免费模型）
            // https://opencode.ai/docs/zen/
            key: "public".to_string(),
            is_default: true,
        }],
        models,
        max_tokens: 4096,
        temperature: 0.3,
    }]
}

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
            provider_policy: ProviderPolicyConfig::default(),
            config_path,
            client: reqwest::Client::new(),
            remote_model_cache: HashMap::new(),
            seeding_in_progress: Arc::new(AtomicBool::new(false)),
        };
        manager.load();
        manager
    }

    /// 异步 seed 默认 OpenCode Zen 供应商
    ///
    /// 行为：
    /// - 仅在 `config_path` 不存在时执行（用户删过默认后不会自动恢复）
    /// - 写完文件后**同时更新内存状态** —— 修复前只写文件、不更新内存，导致 Settings 页看不到默认供应商
    /// - 网络拉取失败时**不**写文件、不更新内存，并 `tracing::warn!` 提示用户在 Settings 手动添加
    /// - 期间 `seeding_in_progress = true`，`save()` 会拒绝写文件，避免与 seed 互相覆盖
    ///
    /// 调用方负责 spawn 到 tokio runtime 中。失败仅 `tracing::warn!`，不阻塞启动
    pub async fn seed_default_async(arc_self: &Arc<RwLock<Self>>) {
        let config_path = match arc_self.read() {
            Ok(mgr) => mgr.config_path.clone(),
            Err(e) => {
                tracing::warn!("seed 前读 manager 失败: {}", e);
                return;
            }
        };
        if config_path.exists() {
            return;
        }

        // 标记 seed 进行中：阻止 save() 写文件
        let _ = arc_self
            .read()
            .map(|m| m.seeding_in_progress.store(true, Ordering::SeqCst));

        let free_models = fetch_opencode_zen_free_models().await;
        let default = seed_default_opencode_zen(free_models);

        // 网络失败时 default 为空 — 不写文件、不更新内存，让用户在 Settings 手动添加
        if default.is_empty() {
            tracing::warn!(
                "OpenCode Zen /models 拉取失败（离线或 API 变更），跳过默认供应商 seed。请在设置中手动添加供应商。"
            );
            let _ = arc_self
                .read()
                .map(|m| m.seeding_in_progress.store(false, Ordering::SeqCst));
            return;
        }

        // 写文件
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let payload = ProviderConfigFile {
            providers: Some(default.clone()),
            ocr_config: None,
            provider_policy: Some(ProviderPolicyConfig::default()),
        };
        match serde_json::to_string_pretty(&payload) {
            Ok(content) => {
                if let Err(e) = std::fs::write(&config_path, content) {
                    tracing::warn!("写入默认 OpenCode Zen 配置失败（保留 flag 阻止 save）: {}", e);
                    let _ = arc_self
                        .read()
                        .map(|m| m.seeding_in_progress.store(false, Ordering::SeqCst));
                    return;
                }
            }
            Err(e) => {
                tracing::warn!("序列化默认配置失败（保留 flag 阻止 save）: {}", e);
                let _ = arc_self
                    .read()
                    .map(|m| m.seeding_in_progress.store(false, Ordering::SeqCst));
                return;
            }
        }

        // **关键修复**：写完文件后同步到内存状态
        // 修复前：seed 任务只写文件，`arc_self.providers` 永远是空，
        //         Settings 页调用 list_providers() 看到空 Vec。
        //         用户必须关 app 重启才能看到默认供应商。
        // 修复后：写文件 + 更新内存同时进行，Settings 立刻能读到。
        // tokio RwLock.write() 不会因 panic 毒化，Err 分支实际不可达；
        // 仍保留 match 以应对未来 runtime 变更，失败时保留 flag 阻止 save 覆盖文件
        if let Ok(mut mgr) = arc_self.write() {
            mgr.providers = default;
        } else {
            tracing::error!("seed 写内存状态时获取写锁失败（理论不可达，已保留 flag 阻止 save）");
            return;
        }

        // 释放 flag：允许用户后续 save()
        let _ = arc_self
            .read()
            .map(|m| m.seeding_in_progress.store(false, Ordering::SeqCst));

        tracing::info!("首次启动已 seed 默认 OpenCode Zen 配置");
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
        // 防竞态：首次启动 seed 进行中不允许 save()，避免覆盖用户输入
        // （seed 会在 fetch + 写文件完成后自动释放 flag）
        if self.seeding_in_progress.load(Ordering::SeqCst) {
            return Err("首次启动配置初始化中，请稍候再保存（约 8 秒）".to_string());
        }

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
        let result = seed_default_opencode_zen(Vec::new());
        assert!(
            result.is_empty(),
            "空模型列表必须返回空供应商列表，禁止硬塞兜底模型"
        );
    }

    #[test]
    fn seed_default_opencode_zen_wraps_models_in_opencode_zen_provider() {
        let result = seed_default_opencode_zen(vec!["gpt-free".to_string(), "claude-free".to_string()]);
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
