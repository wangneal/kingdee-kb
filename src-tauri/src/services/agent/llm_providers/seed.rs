//! 首次启动 seed 逻辑 + 供应商/策略校验工具函数
//!
//! 负责从 OpenCode Zen 拉取免费模型并写入默认配置，
//! 以及远程模型缓存、解析、校验等辅助功能。

use std::collections::{hash_map::DefaultHasher, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use super::types::*;

/// OpenCode Zen 接口基础地址
pub(crate) const OPENCODE_ZEN_BASE_URL: &str = "https://opencode.ai/zen/v1";

/// 远程模型缓存有效期
pub(crate) const REMOTE_MODEL_CACHE_TTL: Duration = Duration::from_secs(300);

// ─── seed 相关 ───

/// 异步从 OpenCode Zen `/v1/models` 拉取所有带 `-free` 后缀的模型名
/// 使用 "public" key（OpenCode Zen 免费模型约定的公共 key）
///
/// 错误处理：网络/解析失败时返回空 Vec，由 `seed_default_async` 决定是否继续。
/// 失败会记 debug 日志便于排查"为什么没拉到模型"——网络失败、防火墙拦截、API 变更等都能区分。
/// 不会"猜测"兜底模型：拉不到就**不**写默认配置，让用户在 Settings 页面手动添加供应商
pub(crate) async fn fetch_opencode_zen_free_models() -> Vec<String> {
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
pub(crate) fn seed_default_opencode_zen(free_models: Vec<String>) -> Vec<LLMProviderConfig> {
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

impl super::LLMProviderManager {
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

        let mut free_models = fetch_opencode_zen_free_models().await;
        if free_models.is_empty() {
            tracing::info!("OpenCode Zen /models 拉取为空或网络离线，使用内置默认免费模型列表进行初始化");
            free_models = vec![
                "deepseek-v4-flash-free".to_string(),
                "qwen3.6-plus-free".to_string(),
                "minimax-m3-free".to_string(),
                "mimo-v2.5-free".to_string(),
            ];
        }
        let default = seed_default_opencode_zen(free_models);

        if default.is_empty() {
            tracing::warn!(
                "未成功生成默认供应商配置，跳过默认供应商 seed。请在设置中手动添加供应商。"
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
            excluded_image_types: None,
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
}

// ─── 工具函数 ───

/// 默认排除的图片类型：装饰性 image 类（照片/Logo 等），减少噪声和成本
pub(crate) fn default_excluded_image_types() -> Vec<String> {
    vec!["image".to_string()]
}

pub(crate) fn validate_provider_policy(policy: &ProviderPolicyConfig) -> Result<(), String> {
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

pub(crate) fn provider_policy_effect(
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

pub(crate) fn remote_model_cache_key(protocol: &LLMProtocol, base_url: &str, api_key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    format!("{:?}", protocol).hash(&mut hasher);
    base_url.trim().trim_end_matches('/').hash(&mut hasher);
    api_key.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

pub(crate) fn parse_remote_model_names(body: &str) -> Result<Vec<String>, String> {
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

pub(crate) fn models_endpoint_url(base_url: &str) -> Result<String, String> {
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
