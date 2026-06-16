//! Embedding service: text → vector conversion
//!
//! 支持两种模式：
//! - 在线 API: OpenAI / 硅基流动 / 智谱 / 阿里灵积 / Cohere / 自定义 (OpenAI 兼容)
//! - Ollama: 本地 LLM 服务器 (用户自行安装)

use serde::{Deserialize, Serialize};
use std::time::Instant;

// ─── Embedding 提供商 ───

/// Embedding 提供商类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EmbeddingProvider {
    /// Ollama 本地 LLM 服务器
    #[serde(rename = "ollama")]
    Ollama,
    /// OpenAI
    #[serde(rename = "openai")]
    OpenAI,
    /// 硅基流动 (SiliconFlow)
    #[serde(rename = "siliconflow")]
    SiliconFlow,
    /// 智谱 (Zhipu)
    #[serde(rename = "zhipu")]
    Zhipu,
    /// 阿里灵积 (DashScope)
    #[serde(rename = "dashscope")]
    DashScope,
    /// Cohere
    #[serde(rename = "cohere")]
    Cohere,
    /// 自定义 OpenAI 兼容端点（用户填写任意 base_url/model/api_key）
    #[serde(rename = "custom")]
    Custom,
}

impl EmbeddingProvider {
    /// 返回所有可用的提供商列表（含默认配置）
    pub fn all_providers() -> Vec<ProviderInfo> {
        vec![
            ProviderInfo {
                provider: EmbeddingProvider::Ollama,
                name: "Ollama (本地)".to_string(),
                description: "本地 LLM 服务器，需自行安装 Ollama 并拉取 embedding 模型"
                    .to_string(),
                default_base_url: Some("http://localhost:11434".to_string()),
                default_model: Some("nomic-embed-text".to_string()),
                requires_api_key: false,
            },
            ProviderInfo {
                provider: EmbeddingProvider::OpenAI,
                name: "OpenAI".to_string(),
                description: "text-embedding-3-small/large, ada-002".to_string(),
                default_base_url: Some("https://api.openai.com/v1".to_string()),
                default_model: Some("text-embedding-3-small".to_string()),
                requires_api_key: true,
            },
            ProviderInfo {
                provider: EmbeddingProvider::SiliconFlow,
                name: "硅基流动 (SiliconFlow)".to_string(),
                description: "BGE-M3, BGE-large-zh 等国产模型".to_string(),
                default_base_url: Some("https://api.siliconflow.cn/v1".to_string()),
                default_model: Some("BAAI/bge-m3".to_string()),
                requires_api_key: true,
            },
            ProviderInfo {
                provider: EmbeddingProvider::Zhipu,
                name: "智谱 (Zhipu)".to_string(),
                description: "embedding-3 模型".to_string(),
                default_base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
                default_model: Some("embedding-3".to_string()),
                requires_api_key: true,
            },
            ProviderInfo {
                provider: EmbeddingProvider::DashScope,
                name: "阿里灵积 (DashScope)".to_string(),
                description: "text-embedding-v3/v2 模型".to_string(),
                default_base_url: Some(
                    "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
                ),
                default_model: Some("text-embedding-v3".to_string()),
                requires_api_key: true,
            },
            ProviderInfo {
                provider: EmbeddingProvider::Cohere,
                name: "Cohere".to_string(),
                description: "embed-multilingual-v3.0 等多语言模型".to_string(),
                default_base_url: Some("https://api.cohere.com/v2".to_string()),
                default_model: Some("embed-multilingual-v3.0".to_string()),
                requires_api_key: true,
            },
            ProviderInfo {
                provider: EmbeddingProvider::Custom,
                name: "自定义 (OpenAI 兼容)".to_string(),
                description: "任意兼容 OpenAI /embeddings 的端点，自定义 base_url 和模型"
                    .to_string(),
                default_base_url: None,
                default_model: None,
                requires_api_key: true,
            },
        ]
    }
}

/// 提供商信息（用于前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub provider: EmbeddingProvider,
    pub name: String,
    pub description: String,
    pub default_base_url: Option<String>,
    pub default_model: Option<String>,
    pub requires_api_key: bool,
}

/// Embedding 远程配置（包括 Ollama）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEmbeddingConfig {
    pub provider: EmbeddingProvider,
    pub api_key: String,
    pub base_url: String,
    pub model_name: String,
}

const DEFAULT_REMOTE_DIMENSION: usize = 1024;

// ─── 模型管理器（简化版：仅管理远程配置）──

/// Embedding 模型生命周期管理器。
///
/// 不再支持本地 ONNX 模型下载，仅管理远程 Embedding 配置（包括 Ollama）。
pub struct ModelManager {
    config_path: std::path::PathBuf,
    /// 远程 Embedding 配置（持久化到磁盘）
    remote_config: Option<RemoteEmbeddingConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingModelConfig {
    /// Embedding 提供商
    pub provider: Option<EmbeddingProvider>,
    /// API 密钥
    pub api_key: Option<String>,
    /// API 基础 URL（为空时使用提供商默认值）
    pub base_url: Option<String>,
    /// 模型名称（为空时使用提供商默认值）
    pub model_name: Option<String>,
}

impl ModelManager {
    pub fn new(model_dir: std::path::PathBuf) -> Self {
        let data_dir = model_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| model_dir.clone());
        let config_path = data_dir.join("embedding_config.json");
        let config = Self::load_embedding_config(&config_path).unwrap_or_default();

        // 构建远程配置（如果有）
        let remote_config = config.provider.as_ref().map(|provider| {
            let provider_info = EmbeddingProvider::all_providers()
                .into_iter()
                .find(|p| p.provider == *provider);

            RemoteEmbeddingConfig {
                provider: provider.clone(),
                api_key: config.api_key.clone().unwrap_or_default(),
                base_url: config
                    .base_url
                    .clone()
                    .or_else(|| {
                        provider_info
                            .as_ref()
                            .and_then(|p| p.default_base_url.clone())
                    })
                    .unwrap_or_default(),
                model_name: config
                    .model_name
                    .clone()
                    .or_else(|| provider_info.as_ref().and_then(|p| p.default_model.clone()))
                    .unwrap_or_default(),
            }
        });

        Self {
            config_path,
            remote_config,
        }
    }

    fn load_embedding_config(path: &std::path::PathBuf) -> Result<EmbeddingModelConfig, String> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| format!("Read embedding config failed: {}", e))?;
        serde_json::from_str(&data).map_err(|e| format!("Parse embedding config failed: {}", e))
    }

    fn save_embedding_config(&self) -> Result<(), String> {
        let config = self.embedding_config();
        let data = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Serialize embedding config failed: {}", e))?;
        std::fs::write(&self.config_path, data)
            .map_err(|e| format!("Write embedding config failed: {}", e))
    }

    pub fn embedding_config(&self) -> EmbeddingModelConfig {
        EmbeddingModelConfig {
            provider: self.remote_config.as_ref().map(|c| c.provider.clone()),
            api_key: self.remote_config.as_ref().map(|c| c.api_key.clone()),
            base_url: self.remote_config.as_ref().map(|c| c.base_url.clone()),
            model_name: self.remote_config.as_ref().map(|c| c.model_name.clone()),
        }
    }

    /// 设置 Embedding 提供商配置
    pub fn set_remote_config(
        &mut self,
        config: Option<RemoteEmbeddingConfig>,
    ) -> Result<(), String> {
        self.remote_config = config;
        self.save_embedding_config()
    }

    /// 获取当前远程配置
    pub fn remote_config(&self) -> Option<&RemoteEmbeddingConfig> {
        self.remote_config.as_ref()
    }

    /// 远程模式始终就绪
    pub fn is_ready(&self) -> bool {
        self.remote_config.is_some()
    }
}

// ─── Embedding 服务 ───

/// Embedding service for text vectorization
///
/// 仅支持远程 API 模式（在线 API + Ollama）。
pub struct EmbeddingService {
    /// 远程 Embedding 配置
    remote_config: Option<RemoteEmbeddingConfig>,
    /// HTTP 客户端（连接池复用，全局共享）
    client: reqwest::Client,
    /// Cached embedding dimension
    cached_dimension: usize,
    /// 上次使用时间（用于空闲超时释放）
    last_used: Instant,
}

impl EmbeddingService {
    /// 创建空的 EmbeddingService（需通过 set_remote_config 配置后才能使用）
    pub fn empty() -> Self {
        Self {
            remote_config: None,
            client: reqwest::Client::new(),
            cached_dimension: DEFAULT_REMOTE_DIMENSION,
            last_used: Instant::now(),
        }
    }

    /// 配置 Embedding 提供商
    pub fn set_remote_config(&mut self, config: Option<RemoteEmbeddingConfig>) {
        if config.is_some() {
            self.cached_dimension = DEFAULT_REMOTE_DIMENSION;
        }
        self.remote_config = config;
    }

    /// 获取当前远程配置
    pub fn remote_config(&self) -> Option<&RemoteEmbeddingConfig> {
        self.remote_config.as_ref()
    }

    /// 是否已配置
    pub fn is_ready(&self) -> bool {
        self.remote_config.is_some()
    }

    /// 获取 embedding 维度
    pub fn dimension(&self) -> usize {
        self.cached_dimension
    }

    /// 距上次使用的空闲时间（秒）
    pub fn idle_seconds(&self) -> u64 {
        self.last_used.elapsed().as_secs()
    }

    /// 获取全局 HTTP 客户端（连接池复用）
    pub fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    /// 同步批量 Embedding（内部桥接到异步 remote_embed_batch）
    ///
    /// 调用者负责在外部持有 RwLock read guard，本方法仅读取 self 字段。
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        let config = self
            .remote_config
            .as_ref()
            .ok_or_else(|| "Embedding 未配置，请先在设置中选择 Embedding 提供商".to_string())?;
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| "无法获取异步运行时".to_string())?;
        handle.block_on(remote_embed_batch(&self.client, config, texts))
    }

    /// 同步单条 Embedding（内部桥接到异步 remote_embed）
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>, String> {
        let config = self
            .remote_config
            .as_ref()
            .ok_or_else(|| "Embedding 未配置，请先在设置中选择 Embedding 提供商".to_string())?;
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| "无法获取异步运行时".to_string())?;
        handle.block_on(remote_embed(&self.client, config, text))
    }
}

// ─── 远程 Embedding API 调用（异步）──

/// 在线 Embedding API 响应结构（OpenAI 兼容格式）
#[derive(Debug, Deserialize)]
struct EmbeddingApiResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

/// Ollama Embedding API 响应结构
#[derive(Debug, Deserialize)]
struct OllamaEmbeddingResponse {
    embeddings: Vec<Vec<f32>>,
}

/// 调用 Embedding API
///
/// 根据 provider 类型自动选择 OpenAI 兼容格式或 Ollama 原生格式。
/// `client` 应传入全局共享的 reqwest::Client 以复用连接池。
pub async fn remote_embed(
    client: &reqwest::Client,
    config: &RemoteEmbeddingConfig,
    text: &str,
) -> Result<Vec<f32>, String> {
    match config.provider {
        EmbeddingProvider::Ollama => ollama_embed(client, config, &[text])
            .await?
            .into_iter()
            .next()
            .ok_or("Ollama 未返回 embedding".to_string()),
        _ => openai_embed(client, config, &[text])
            .await?
            .into_iter()
            .next()
            .ok_or("API 未返回 embedding".to_string()),
    }
}

/// 批量调用 Embedding API
///
/// 根据 provider 类型自动选择 OpenAI 兼容格式或 Ollama 原生格式。
/// `client` 应传入全局共享的 reqwest::Client 以复用连接池。
pub async fn remote_embed_batch(
    client: &reqwest::Client,
    config: &RemoteEmbeddingConfig,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>, String> {
    match config.provider {
        EmbeddingProvider::Ollama => ollama_embed(client, config, texts).await,
        _ => openai_embed(client, config, texts).await,
    }
}

/// OpenAI 兼容格式 Embedding 调用
async fn openai_embed(
    client: &reqwest::Client,
    config: &RemoteEmbeddingConfig,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>, String> {
    let url = format!("{}/embeddings", config.base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": config.model_name,
        "input": texts
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Embedding 请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable>".to_string());
        return Err(format!("Embedding API 错误 ({}): {}", status, body_text));
    }

    let json: EmbeddingApiResponse = response
        .json()
        .await
        .map_err(|e| format!("解析 Embedding 响应失败: {}", e))?;

    // 按 index 排序以保证顺序正确
    let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
    for (i, data) in json.data.into_iter().enumerate() {
        if i < results.len() {
            results[i] = Some(data.embedding);
        }
    }

    results
        .into_iter()
        .enumerate()
        .map(|(i, v)| v.ok_or_else(|| format!("缺少 index {} 的 embedding", i)))
        .collect()
}

/// Ollama 原生格式 Embedding 调用
///
/// API: POST {base_url}/api/embed
/// 请求体: {"model": "nomic-embed-text", "input": ["text1", "text2"]}
/// 响应体: {"embeddings": [[0.1, 0.2, ...], ...]}
async fn ollama_embed(
    client: &reqwest::Client,
    config: &RemoteEmbeddingConfig,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>, String> {
    let url = format!("{}/api/embed", config.base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": config.model_name,
        "input": texts
    });

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama Embedding 请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable>".to_string());
        return Err(format!("Ollama API 错误 ({}): {}", status, body_text));
    }

    let json: OllamaEmbeddingResponse = response
        .json()
        .await
        .map_err(|e| format!("解析 Ollama Embedding 响应失败: {}", e))?;

    if json.embeddings.len() != texts.len() {
        return Err(format!(
            "Ollama 返回 {} 个 embedding，但请求了 {} 个",
            json.embeddings.len(),
            texts.len()
        ));
    }

    Ok(json.embeddings)
}

/// 计算两个向量的余弦相似度
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
