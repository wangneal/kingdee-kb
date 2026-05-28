//! Embedding service: text 鈫?vector conversion via fastembed-rs
//!
//! Uses bge-small-zh-v1.5 (512-dim) for Chinese text embeddings.
//! Model is auto-downloaded on first use to `~/.cache/huggingface/hub/`.
//!
//! NOTE: HuggingFace is blocked in China. The model download requires:
//!   - Setting HF_ENDPOINT=https://hf-mirror.com (may not support range requests)
//!   - Or pre-downloading model files to ~/.cache/huggingface/

use fastembed::{
    EmbeddingModel, InitOptions, InitOptionsUserDefined, TextEmbedding, UserDefinedEmbeddingModel,
};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ─── 在线 Embedding 提供商 ───

/// Embedding 提供商类型 — 决定使用本地模型还是在线 API
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EmbeddingProvider {
    /// 本地 ONNX 模型（fastembed-rs）
    #[serde(rename = "local")]
    Local,
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
}

impl EmbeddingProvider {
    /// 返回所有可用的提供商列表（含默认配置）
    pub fn all_providers() -> Vec<ProviderInfo> {
        vec![
            ProviderInfo {
                provider: EmbeddingProvider::Local,
                name: "本地模型".to_string(),
                description: "使用本地 ONNX 模型（BGE/MiniLM），无需网络".to_string(),
                default_base_url: None,
                default_model: Some("bge-small-zh-v1.5".to_string()),
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

/// 在线 Embedding 远程配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEmbeddingConfig {
    pub provider: EmbeddingProvider,
    pub api_key: String,
    pub base_url: String,
    pub model_name: String,
}

/// HuggingFace mirror list, ordered by likely speed in China.
/// `hf-hub` (used internally by fastembed) reads the `HF_ENDPOINT` env var
/// to determine which mirror to download from.
///
/// Note: hf-mirror.com doesn't support HTTP Range/Content-Range headers,
/// which hf-hub requires for its download logic. Some attempts may fail
/// with "Header Content-Range is missing". This is normal 鈥?the fallback
/// mirror will be tried next.
const HF_MIRRORS: &[Option<&str>] = &[
    Some("https://hf-mirror.com"), // Official HF Chinese mirror (fast, no Range)
    None,                          // Default (huggingface.co)
];

/// Expected total download size for bge-small-zh-v1.5 (model + tokenizer + config).
/// Used for progress estimation.
const EXPECTED_MODEL_BYTES: u64 = 95_000_000;

const DEFAULT_BGE_DIMENSION: usize = 512;
const DEFAULT_MINILM_DIMENSION: usize = 384;
const DEFAULT_REMOTE_DIMENSION: usize = 1024;

/// Start a background polling thread that estimates model download progress
/// by scanning the HuggingFace cache directory.
///
/// Returns a handle to the progress value (0鈥?9 while downloading, 100 = done).
/// The caller must set `stop` to `true` when the download finishes.
pub fn start_download_progress_polling(
    model: &EmbeddingModel,
    progress: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
) {
    let Some(cache_dir) = model_hf_cache_dir(model) else {
        tracing::warn!("Cannot determine HF cache dir, progress unavailable");
        return;
    };

    tracing::info!("Starting progress polling for {:?}", cache_dir);

    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let total = sum_dir_size(&cache_dir);
            let pct = if total >= EXPECTED_MODEL_BYTES {
                99u32
            } else if EXPECTED_MODEL_BYTES > 0 {
                ((total as f64 / EXPECTED_MODEL_BYTES as f64) * 99.0) as u32
            } else {
                0u32
            };
            progress.store(pct.min(99), Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(500));
        }
        // Thread exiting 鈥?caller will set progress to 100 externally
    });
}

/// Recursively sum file sizes under a directory tree.
fn sum_dir_size(dir: &PathBuf) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += sum_dir_size(&path);
            } else if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                }
            }
        }
    }
    total
}

/// Get the HuggingFace cache directory for a given model.
///
/// Uses the same logic as `hf-hub` crate:
///   1. `$HF_HOME/hub` (if `HF_HOME` env var is set)
///   2. Otherwise `dirs::cache_dir()/huggingface/hub`
///
/// On Windows `dirs::cache_dir()` returns `%LOCALAPPDATA%`
/// (typically `C:\Users\<you>\AppData\Local\huggingface\hub\`).
pub fn model_hf_cache_dir(model: &EmbeddingModel) -> Option<PathBuf> {
    let cache_root = std::env::var("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::cache_dir()
                .unwrap_or_else(|| {
                    // Fallback: ~/.cache (Linux/macOS) or %USERPROFILE%\.cache
                    let home = std::env::var("HOME")
                        .or_else(|_| std::env::var("USERPROFILE"))
                        .unwrap_or_else(|_| ".".to_string());
                    PathBuf::from(home).join(".cache")
                })
                .join("huggingface")
        })
        .join("hub");
    let repo_id = match model {
        EmbeddingModel::BGESmallZHV15 => "BAAI/bge-small-zh-v1.5",
        EmbeddingModel::AllMiniLML6V2 => "sentence-transformers/all-MiniLM-L6-v2",
        _ => return None,
    };
    let cache_key = format!("models--{}", repo_id.replace('/', "--"));
    Some(cache_root.join(cache_key))
}

/// Managed state for the embedding model lifecycle.
pub struct ModelManager {
    model_dir: PathBuf,
    config_path: PathBuf,
    custom_model_dir: Option<PathBuf>,
    model: Option<TextEmbedding>,
    is_ready: bool,
    /// 在线 Embedding 配置（持久化到磁盘）
    remote_config: Option<RemoteEmbeddingConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingModelConfig {
    /// 自定义本地模型目录（仅 Local 提供商使用）
    pub custom_model_dir: Option<String>,
    /// Embedding 提供商（None 表示使用默认的 Local）
    pub provider: Option<EmbeddingProvider>,
    /// 在线 API 密钥（仅在线提供商需要）
    pub api_key: Option<String>,
    /// 在线 API 基础 URL（为空时使用提供商默认值）
    pub base_url: Option<String>,
    /// 在线模型名称（为空时使用提供商默认值）
    pub model_name: Option<String>,
}

impl ModelManager {
    /// Create a new ModelManager with the given model cache directory
    pub fn new(model_dir: PathBuf) -> Self {
        let data_dir = model_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| model_dir.clone());
        let config_path = data_dir.join("embedding_config.json");
        let config = Self::load_embedding_config(&config_path).unwrap_or_default();

        // 构建远程配置（如果有）
        let remote_config = match config.provider {
            Some(ref provider) if *provider != EmbeddingProvider::Local => {
                let provider_info = EmbeddingProvider::all_providers()
                    .into_iter()
                    .find(|p| p.provider == *provider);

                Some(RemoteEmbeddingConfig {
                    provider: provider.clone(),
                    api_key: config.api_key.clone().unwrap_or_default(),
                    base_url: config
                        .base_url
                        .clone()
                        .or_else(|| provider_info.as_ref().and_then(|p| p.default_base_url.clone()))
                        .unwrap_or_default(),
                    model_name: config
                        .model_name
                        .clone()
                        .or_else(|| provider_info.as_ref().and_then(|p| p.default_model.clone()))
                        .unwrap_or_default(),
                })
            }
            _ => None,
        };

        Self {
            model_dir,
            config_path,
            custom_model_dir: config.custom_model_dir.map(PathBuf::from),
            model: None,
            is_ready: remote_config.is_some(), // 远程模式立即就绪
            remote_config,
        }
    }

    fn load_embedding_config(path: &PathBuf) -> Result<EmbeddingModelConfig, String> {
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
            custom_model_dir: self
                .custom_model_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            provider: self.remote_config.as_ref().map(|c| c.provider.clone()),
            api_key: self.remote_config.as_ref().map(|c| c.api_key.clone()),
            base_url: self.remote_config.as_ref().map(|c| c.base_url.clone()),
            model_name: self.remote_config.as_ref().map(|c| c.model_name.clone()),
        }
    }

    pub fn set_custom_model_dir(&mut self, dir: Option<String>) -> Result<(), String> {
        if let Some(dir) = dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let path = PathBuf::from(dir);
            Self::load_user_defined_from_dir(&path)?;
            self.custom_model_dir = Some(path);
        } else {
            self.custom_model_dir = None;
        }

        self.model = None;
        self.is_ready = false;
        self.save_embedding_config()
    }

    /// 设置在线 Embedding 提供商配置
    ///
    /// 传入 None 切换回本地模式。
    pub fn set_remote_config(&mut self, config: Option<RemoteEmbeddingConfig>) -> Result<(), String> {
        if config.is_some() {
            // 切换到远程模式时，释放本地模型
            self.model = None;
            self.is_ready = true; // 远程模式立即就绪
        } else {
            self.is_ready = false; // 切换回本地模式需要重新初始化
        }
        self.remote_config = config;
        self.save_embedding_config()
    }

    /// 获取当前远程配置（如果有）
    pub fn remote_config(&self) -> Option<&RemoteEmbeddingConfig> {
        self.remote_config.as_ref()
    }

    /// Initialize the embedding model (downloads on first use).
    ///
    /// Tries multiple HuggingFace mirrors in sequence. If the model is already
    /// cached in `~/.cache/huggingface/hub/`, this completes instantly regardless
    /// of mirror availability.
    pub fn init(&mut self) -> Result<(), String> {
        if self.is_ready {
            return Ok(());
        }

        // 远程模式不需要初始化本地模型
        if self.remote_config.is_some() {
            self.is_ready = true;
            return Ok(());
        }

        std::fs::create_dir_all(&self.model_dir)
            .map_err(|e| format!("Failed to create model directory: {}", e))?;

        if let Some(custom_dir) = self.custom_model_dir.clone() {
            tracing::info!("Loading custom embedding model from {:?}", custom_dir);
            match Self::load_user_defined_from_dir(&custom_dir) {
                Ok(text_emb) => {
                    tracing::info!("Custom embedding model loaded!");
                    self.model = Some(text_emb);
                    self.is_ready = true;
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("Custom model load failed: {}", e);
                }
            }
        }

        if let Some(base_dir) = Self::bundled_bge_model_dir() {
            tracing::info!("Loading bundled embedding model from {:?}", base_dir);
            match Self::load_user_defined_from_dir(&base_dir) {
                Ok(text_emb) => {
                    tracing::info!("Bundled embedding model loaded!");
                    self.model = Some(text_emb);
                    self.is_ready = true;
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("Bundled model load failed: {}", e);
                }
            }
        }

        // Step 0: Try loading from local cache first (no network required).
        // If the model was previously downloaded and cached, fastembed can load
        // it directly without any network access. This handles the case where
        // the user has a cached model but is currently offline or behind a
        // firewall that blocks HuggingFace.
        tracing::info!("Checking for locally cached model...");
        for model in &[EmbeddingModel::BGESmallZHV15, EmbeddingModel::AllMiniLML6V2] {
            match Self::load_user_defined_from_cache(model) {
                Ok(Some(text_emb)) => {
                    tracing::info!("Successfully loaded {:?} from local files!", model);
                    self.model = Some(text_emb);
                    self.is_ready = true;
                    return Ok(());
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("Local UserDefined load failed for {:?}: {}", model, e);
                }
            }
        }

        // Step 1: Try downloading from mirrors (network required)
        let model = Self::try_init_with_mirrors(EmbeddingModel::BGESmallZHV15)
            .or_else(|_| {
                tracing::warn!("BGE model unavailable, trying default model...");
                Self::try_init_with_mirrors(EmbeddingModel::AllMiniLML6V2)
            })
            .map_err(|e| {
                format!(
                    "Failed to initialize any embedding model: {}\n\
                 Hint: The first download may take a few minutes. \
                 Try setting HF_ENDPOINT=https://hf-mirror.com in your environment, \
                 or pre-download model files to ~/.cache/huggingface/.",
                    e
                )
            })?;

        self.model = Some(model);
        self.is_ready = true;

        Ok(())
    }

    /// Try to initialize a model by attempting each mirror in `HF_MIRRORS`.
    ///
    /// Saves and restores the original `HF_ENDPOINT` env var around each attempt.
    /// If the model is already cached, all mirrors succeed instantly.
    fn try_init_with_mirrors(model: EmbeddingModel) -> Result<TextEmbedding, String> {
        // Save the original HF_ENDPOINT so we can restore it
        let original_hf_endpoint = std::env::var("HF_ENDPOINT").ok();
        let mut last_err = String::new();

        for (i, mirror) in HF_MIRRORS.iter().enumerate() {
            // Set HF_ENDPOINT for this mirror
            let label = match mirror {
                Some(url) => {
                    tracing::info!("Trying mirror {}: {}", i + 1, url);
                    std::env::set_var("HF_ENDPOINT", url);
                    url
                }
                None => {
                    tracing::info!("Trying mirror {}: default (huggingface.co)", i + 1);
                    match &original_hf_endpoint {
                        Some(val) => std::env::set_var("HF_ENDPOINT", val),
                        None => std::env::remove_var("HF_ENDPOINT"),
                    }
                    "default (huggingface.co)"
                }
            };

            match TextEmbedding::try_new(
                InitOptions::new(model.clone()).with_show_download_progress(true),
            ) {
                Ok(text_emb) => {
                    Self::restore_hf_endpoint(&original_hf_endpoint);
                    return Ok(text_emb);
                }
                Err(e) => {
                    last_err = format!("{} failed: {}", label, e);
                    tracing::warn!("Mirror {}: {} failed: {}", i + 1, label, e);

                    // Check if model files were actually cached despite the error.
                    // hf-mirror.com may successfully download the file but fail
                    // hf-hub's Content-Range validation. In that case the blob
                    // files are on disk and usable 鈥?no need to re-download.
                    if Self::has_cached_model_files(&model) {
                        tracing::info!(
                            "Model files found in cache despite error. \
                             Retrying with local UserDefined loader..."
                        );
                        match Self::load_user_defined_from_cache(&model) {
                            Ok(Some(text_emb)) => {
                                tracing::info!("Successfully loaded from local cache!");
                                return Ok(text_emb);
                            }
                            Ok(None) => {
                                let msg = format!(
                                    "cached blobs present ({}) but complete local files were not found",
                                    label
                                );
                                tracing::warn!("{}", msg);
                                last_err = msg;
                            }
                            Err(e2) => {
                                let msg = format!(
                                    "cached files present ({}) but UserDefined reload failed: {}",
                                    label, e2
                                );
                                tracing::warn!("{}", msg);
                                last_err = msg;
                            }
                        }
                    } else {
                        tracing::info!("No cached files found. Clearing partial cache...");
                        if let Some(cache_dir) = model_hf_cache_dir(&model) {
                            let _ = std::fs::remove_dir_all(&cache_dir);
                        }
                    }
                }
            }
        }

        // Restore original HF_ENDPOINT on failure too
        Self::restore_hf_endpoint(&original_hf_endpoint);

        // Last resort: try downloading model files directly using plain HTTP
        // from the first mirror (bypasses hf-hub's Content-Range requirement).
        // Then load using `try_new_from_user_defined` which bypasses hf-hub entirely.
        tracing::warn!("All mirrors failed. Trying direct HTTP download...");
        if Self::download_model_direct(&model) {
            tracing::info!("Direct download succeeded, loading via UserDefined...");
            match Self::load_user_defined_from_cache(&model) {
                Ok(Some(text_emb)) => {
                    tracing::info!("Model loaded via UserDefined!");
                    return Ok(text_emb);
                }
                Ok(None) => {
                    last_err = "downloaded files not found in local cache".to_string();
                    tracing::error!("{}", last_err);
                }
                Err(e) => {
                    last_err = format!("UserDefined load failed: {}", e);
                    tracing::error!("{}", last_err);
                }
            }
        }

        Err(format!(
            "All {} mirror(s) failed for model {:?}. Last error: {}",
            HF_MIRRORS.len(),
            model,
            last_err
        ))
    }

    /// Download model files directly from HuggingFace mirror using plain HTTP.
    /// Bypasses hf-hub's Content-Range validation 鈥?works with mirrors that
    /// don't support Range headers (like hf-mirror.com).
    ///
    /// Tries multiple possible file paths and saves to all expected locations.
    fn download_model_direct(model: &EmbeddingModel) -> bool {
        let Some(cache_dir) = model_hf_cache_dir(model) else {
            return false;
        };
        let repo_id = match model {
            EmbeddingModel::BGESmallZHV15 => "Xenova/bge-small-zh-v1.5",
            EmbeddingModel::AllMiniLML6V2 => "sentence-transformers/all-MiniLM-L6-v2",
            _ => return false,
        };
        let base_url = "https://hf-mirror.com";

        // Different models store ONNX files in different locations on HF.
        // BGE: Xenova repo uses onnx/model.onnx
        // MiniLM: sentence-transformers uses onnx/model.onnx (also has root model.onnx in qdrant variant)
        //
        // We need both source paths (where to download from) and dest paths (where to save to).
        // For model.onnx, we always save a copy at root "model.onnx" so that UserDefined
        // loading code (which reads base_dir.join("model.onnx")) can find it.
        let onnx_source_paths: &[&str] = match model {
            EmbeddingModel::BGESmallZHV15 => &["onnx/model.onnx"],
            EmbeddingModel::AllMiniLML6V2 => &["onnx/model.onnx", "model.onnx"],
            _ => &["model.onnx"],
        };
        // Dest paths: save to original location AND root (for easy loading)
        let onnx_dest_paths: &[&str] = match model {
            EmbeddingModel::BGESmallZHV15 => &["onnx/model.onnx", "model.onnx"],
            EmbeddingModel::AllMiniLML6V2 => &["onnx/model.onnx", "model.onnx"],
            _ => &["model.onnx"],
        };

        // Download files using ureq (streaming, no body size limit)
        // Model ONNX files can be 90MB+, need to bypass ureq's 10MB default.
        let files: &[(&str, &[&str], &[&str])] = &[
            ("config.json", &["config.json"], &["config.json"]),
            ("tokenizer.json", &["tokenizer.json"], &["tokenizer.json"]),
            (
                "tokenizer_config.json",
                &["tokenizer_config.json"],
                &["tokenizer_config.json"],
            ),
            (
                "special_tokens_map.json",
                &["special_tokens_map.json"],
                &["special_tokens_map.json"],
            ),
            ("model.onnx", onnx_source_paths, onnx_dest_paths),
        ];

        tracing::info!("Direct download to {:?}", cache_dir);
        let snapshots_dir = cache_dir.join("snapshots").join("main");
        let _ = std::fs::create_dir_all(&snapshots_dir);

        // Create refs/main with the actual commit hash so hf-hub can find the cache
        let _ = std::fs::create_dir_all(cache_dir.join("refs"));
        let commit_hash = "c9745ed1d9f207416be6d2e6f8de32d1f16199bf";
        let _ = std::fs::write(cache_dir.join("refs").join("main"), commit_hash);
        // Also create snapshots/{commit_hash}/ directory for hf-hub
        let snapshots_commit_dir = cache_dir.join("snapshots").join(commit_hash);
        let _ = std::fs::create_dir_all(&snapshots_commit_dir);

        let mut all_success = true;

        for (name, source_paths, dest_paths) in files {
            let mut body: Option<Vec<u8>> = None;
            for src in *source_paths {
                let url = format!("{}/{}/resolve/main/{}", base_url, repo_id, src);
                tracing::info!("  trying {} from {} ...", name, src);
                // Stream download in chunks to avoid ureq's 10MB limit
                match download_file_chunked(&url) {
                    Ok(data) => {
                        tracing::info!("  downloaded {} ({} bytes)", name, data.len());
                        body = Some(data);
                        break;
                    }
                    Err(e) => tracing::warn!("    failed: {}", e),
                }
            }

            let Some(data) = body else {
                tracing::error!("  FAILED {} (all sources exhausted)", name);
                all_success = false;
                continue;
            };

            for dest in *dest_paths {
                let full_path = snapshots_dir.join(dest);
                if let Some(parent) = full_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&full_path, &data) {
                    tracing::error!("    write to {} failed: {}", dest, e);
                } else {
                    tracing::info!("  saved to {}", dest);
                }
                // Also save to snapshots/{commit_hash}/ for hf-hub
                let commit_path = snapshots_commit_dir.join(dest);
                if let Some(parent) = commit_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&commit_path, &data);
                // Also save to blobs/{sha256} for hf-hub's cache lookup.
                // Then create a hardlink from snapshot -> blob (hf-hub requires
                // snapshots/ files to be symlinks or hardlinks to blobs/{hash}).
                let hash = sha2_hex(&data);
                let blob_path = cache_dir.join("blobs").join(&hash);
                if !blob_path.exists() {
                    if let Some(parent) = blob_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&blob_path, &data) {
                        tracing::error!("    blob write failed: {}", e);
                    } else {
                        tracing::info!("  blob saved");
                    }
                }
                // Replace snapshot file with hardlink to blob
                let _ = std::fs::remove_file(&full_path);
                if let Err(e) = std::fs::hard_link(&blob_path, &full_path) {
                    // Hardlink failed (cross-device, etc.) 鈥?just re-copy
                    tracing::warn!("    hardlink failed (non-fatal): {}", e);
                    let _ = std::fs::write(&full_path, &data);
                } else {
                    tracing::info!("  hardlinked to blob");
                }
            }
        }
        all_success
    }

    // ─── Model discovery & loading ───

    fn bundled_bge_model_dir() -> Option<PathBuf> {
        let relative = PathBuf::from("resources")
            .join("models")
            .join("bge-small-zh-v1.5");

        let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&relative);
        if dev_dir.join("model.onnx").exists() && dev_dir.join("tokenizer.json").exists() {
            return Some(dev_dir);
        }

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));

        if let Some(exe_dir) = exe_dir {
            let candidates = [
                exe_dir.join(&relative),
                exe_dir.join("models").join("bge-small-zh-v1.5"),
                exe_dir
                    .join("..")
                    .join("Resources")
                    .join("models")
                    .join("bge-small-zh-v1.5"),
            ];

            for candidate in candidates {
                if candidate.join("model.onnx").exists()
                    && candidate.join("tokenizer.json").exists()
                {
                    return Some(candidate);
                }
            }
        }

        None
    }

    fn load_user_defined_from_dir(base_dir: &PathBuf) -> Result<TextEmbedding, String> {
        let tokenizer_path = base_dir.join("tokenizer.json");
        let onnx_path = [
            base_dir.join("model.onnx"),
            base_dir.join("onnx").join("model.onnx"),
        ]
        .into_iter()
        .find(|p| p.exists())
        .ok_or_else(|| format!("model.onnx not found under {}", base_dir.display()))?;

        let onnx_bytes = std::fs::read(&onnx_path)
            .map_err(|e| format!("read {} failed: {}", onnx_path.display(), e))?;
        let tokenizer_bytes = std::fs::read(&tokenizer_path)
            .map_err(|e| format!("read {} failed: {}", tokenizer_path.display(), e))?;
        let config_bytes = std::fs::read(base_dir.join("config.json")).unwrap_or_default();
        let tokenizer_config =
            std::fs::read(base_dir.join("tokenizer_config.json")).unwrap_or_default();
        let special_tokens =
            std::fs::read(base_dir.join("special_tokens_map.json")).unwrap_or_default();

        let user_model = UserDefinedEmbeddingModel::new(
            onnx_bytes,
            fastembed::TokenizerFiles {
                tokenizer_file: tokenizer_bytes,
                config_file: config_bytes,
                special_tokens_map_file: special_tokens,
                tokenizer_config_file: tokenizer_config,
            },
        );

        TextEmbedding::try_new_from_user_defined(user_model, InitOptionsUserDefined::new())
            .map_err(|e| format!("UserDefined load from {:?} failed: {}", base_dir, e))
    }

    fn load_user_defined_from_cache(
        model: &EmbeddingModel,
    ) -> Result<Option<TextEmbedding>, String> {
        let Some(cache_dir) = model_hf_cache_dir(model) else {
            return Ok(None);
        };

        let mut candidates = Vec::new();
        candidates.push(cache_dir.join("snapshots").join("main"));

        let refs_main = cache_dir.join("refs").join("main");
        if let Ok(commit) = std::fs::read_to_string(&refs_main) {
            let commit = commit.trim();
            if !commit.is_empty() {
                candidates.push(cache_dir.join("snapshots").join(commit));
            }
        }

        if let Ok(entries) = std::fs::read_dir(cache_dir.join("snapshots")) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    candidates.push(entry.path());
                }
            }
        }

        for base_dir in candidates {
            if !base_dir.join("tokenizer.json").exists() {
                continue;
            }

            if !base_dir.join("model.onnx").exists()
                && !base_dir.join("onnx").join("model.onnx").exists()
            {
                continue;
            }

            tracing::info!("Loading {:?} from local files at {:?}", model, base_dir);

            return Self::load_user_defined_from_dir(&base_dir).map(Some);
        }

        Ok(None)
    }

    fn has_cached_model_files(model: &EmbeddingModel) -> bool {
        let Some(cache_dir) = Self::hf_cache_dir_for(model) else {
            return false;
        };
        let blobs_dir = cache_dir.join("blobs");
        let Ok(entries) = std::fs::read_dir(&blobs_dir) else {
            return false;
        };
        let valid_files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                if let Ok(meta) = e.metadata() {
                    meta.len() > 0 && meta.is_file()
                } else {
                    false
                }
            })
            .collect();
        valid_files.len() >= 3
    }

    /// Restore the original `HF_ENDPOINT` environment variable if it was set.
    fn restore_hf_endpoint(original: &Option<String>) {
        match original {
            Some(val) => std::env::set_var("HF_ENDPOINT", val),
            None => std::env::remove_var("HF_ENDPOINT"),
        }
    }

    /// Get the HuggingFace cache directory for a given model.
    /// Returns `~/.cache/huggingface/hub/models--{org}--{name}/`.
    fn hf_cache_dir_for(model: &EmbeddingModel) -> Option<PathBuf> {
        model_hf_cache_dir(model)
    }

    // ─── Accessors ───

    /// Check if the model is ready for inference
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }

    /// Get a reference to the underlying model
    pub fn model(&self) -> Option<&TextEmbedding> {
        self.model.as_ref()
    }

    /// Get a mutable reference to the underlying model
    pub fn model_mut(&mut self) -> Option<&mut TextEmbedding> {
        self.model.as_mut()
    }

    /// Take ownership of the model, leaving ModelManager without direct access.
    /// Used to transfer the model to EmbeddingService after initialization.
    /// `is_ready` remains true because initialization was already completed.
    pub fn take_model(&mut self) -> Option<TextEmbedding> {
        self.model.take()
    }
}

/// Compute SHA256 hex digest of data (for hf-hub blob naming).
fn sha2_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Download a file from URL, reading in chunks to bypass any response size limits.
pub fn download_file_chunked(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP error: {}", e))?;

    let mut body = response.into_body();
    let mut reader = body.as_reader();
    let mut data = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(e) => return Err(format!("read error: {}", e)),
        }
    }
    Ok(data)
}

/// Embedding service for text vectorization
///
/// 支持两种模式：
/// - Local: 使用本地 ONNX 模型（fastembed-rs）
/// - Remote: 调用在线 Embedding API（OpenAI 兼容格式）
pub struct EmbeddingService {
    /// 本地模型（fastembed）— 仅在 Local 模式下使用
    model: Option<TextEmbedding>,
    /// 在线 Embedding 配置（None = 本地模式）
    remote_config: Option<RemoteEmbeddingConfig>,
    /// HTTP 客户端（用于远程调用）
    client: Option<reqwest::Client>,
    /// Cached embedding dimension (detected via probe embed on model injection)
    /// bge-small-zh-v1.5 = 512, all-MiniLM-L6-v2 = 384
    cached_dimension: usize,
    /// Default batch size for embed_batch
    batch_size: usize,
}

/// Probe the embedding dimension by embedding a short test string.
/// Returns the dimension of the resulting vector.
fn probe_dimension(model: &mut TextEmbedding) -> usize {
    model
        .embed(vec!["probe"], None)
        .ok()
        .and_then(|v| v.into_iter().next())
        .map(|v| v.len())
        .unwrap_or(DEFAULT_BGE_DIMENSION) // default to BGE dimension if probe fails
}

impl EmbeddingService {
    /// Create a new embedding service (requires initialized model)
    pub fn new(mut model: TextEmbedding) -> Self {
        let dim = probe_dimension(&mut model);
        Self {
            model: Some(model),
            remote_config: None,
            client: None,
            cached_dimension: dim,
            batch_size: 64,
        }
    }

    /// Create an EmbeddingService without a model (will fail on embed calls)
    pub fn empty() -> Self {
        Self {
            model: None,
            remote_config: None,
            client: None,
            cached_dimension: DEFAULT_BGE_DIMENSION, // default to BGE dimension
            batch_size: 64,
        }
    }

    /// 配置在线 Embedding 提供商
    pub fn set_remote_config(&mut self, config: Option<RemoteEmbeddingConfig>) {
        if config.is_some() {
            // 切换到远程模式时，释放本地模型
            self.model = None;
            self.client = Some(reqwest::Client::new());
            // 远程模型维度因提供商而异，默认 1024（BGE-M3）
            self.cached_dimension = DEFAULT_REMOTE_DIMENSION;
        } else {
            self.client = None;
        }
        self.remote_config = config;
    }

    /// 获取当前远程配置（如果有）
    pub fn remote_config(&self) -> Option<&RemoteEmbeddingConfig> {
        self.remote_config.as_ref()
    }

    /// 是否为远程模式
    pub fn is_remote(&self) -> bool {
        self.remote_config.is_some()
    }

    /// Embed a single text — 本地模式同步调用，远程模式需使用 embed_text_remote
    pub fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String> {
        if self.remote_config.is_some() {
            return Err("Remote embedding mode: use embed_text_remote() instead".to_string());
        }

        let model = self
            .model
            .as_mut()
            .ok_or("Embedding model not initialized")?;

        let embeddings = model
            .embed(vec![text], None)
            .map_err(|e| format!("Embed failed: {}", e))?;

        embeddings
            .into_iter()
            .next()
            .ok_or("No embedding returned".to_string())
    }

    /// Batch embed multiple texts — 本地模式
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        if self.remote_config.is_some() {
            return Err("Remote embedding mode: use embed_batch_remote() instead".to_string());
        }

        let model = self
            .model
            .as_mut()
            .ok_or("Embedding model not initialized")?;

        let mut all_embeddings = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(self.batch_size) {
            let batch: Vec<&str> = chunk.to_vec();
            let embeddings = model
                .embed(batch, None)
                .map_err(|e| format!("Batch embed failed: {}", e))?;
            all_embeddings.extend(embeddings);
        }

        Ok(all_embeddings)
    }

    /// Check if the service is ready
    pub fn is_ready(&self) -> bool {
        self.model.is_some() || self.remote_config.is_some()
    }

    /// Inject an initialized model into this service.
    /// Called by `init_model` command after ModelManager downloads the model.
    /// Also probes the model to detect its embedding dimension.
    pub fn set_model(&mut self, mut model: TextEmbedding) {
        // 切换到本地模式时清除远程配置
        self.remote_config = None;
        self.client = None;
        self.cached_dimension = probe_dimension(&mut model);
        self.model = Some(model);
    }

    /// Get the embedding dimension (detected via probe embed)
    /// bge-small-zh-v1.5: 512, all-MiniLM-L6-v2: 384, BGE-M3: 1024
    pub fn dimension(&self) -> usize {
        self.cached_dimension
    }
}

// ─── 远程 Embedding API 调用（异步） ───

/// 在线 Embedding API 响应结构（OpenAI 兼容格式）
#[derive(Debug, Deserialize)]
struct EmbeddingApiResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

/// 调用在线 Embedding API（OpenAI 兼容格式）
///
/// 所有支持的提供商（OpenAI、SiliconFlow、Zhipu、DashScope）都使用相同的 API 格式：
/// POST {base_url}/embeddings
/// Cohere 使用 v2 兼容格式。
pub async fn remote_embed(config: &RemoteEmbeddingConfig, text: &str) -> Result<Vec<f32>, String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/embeddings",
        config.base_url.trim_end_matches('/')
    );

    let body = serde_json::json!({
        "model": config.model_name,
        "input": [text]
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Remote embedding request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable>".to_string());
        return Err(format!(
            "Remote embedding API error ({}): {}",
            status, body_text
        ));
    }

    let json: EmbeddingApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse embedding response: {}", e))?;

    json.data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .ok_or("No embedding data in response".to_string())
}

/// 批量调用在线 Embedding API（OpenAI 兼容格式）
///
/// 一次请求嵌入多个文本，减少网络开销。
pub async fn remote_embed_batch(
    config: &RemoteEmbeddingConfig,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>, String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/embeddings",
        config.base_url.trim_end_matches('/')
    );

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
        .map_err(|e| format!("Remote batch embedding request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable>".to_string());
        return Err(format!(
            "Remote embedding API error ({}): {}",
            status, body_text
        ));
    }

    let json: EmbeddingApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse embedding response: {}", e))?;

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
        .map(|(i, v)| v.ok_or_else(|| format!("Missing embedding for index {}", i)))
        .collect()
}

/// Compute cosine similarity between two vectors
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

// 鈹€鈹€ Tests 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Loads the locally cached embedding model from the user's HuggingFace cache"]
    fn test_load_bge_from_local_cache_user_defined() {
        let model = ModelManager::load_user_defined_from_cache(&EmbeddingModel::BGESmallZHV15)
            .expect("local cache loader should not fail");
        assert!(model.is_some(), "BGE local cache files were not found");
    }

    #[test]
    fn test_load_bundled_bge_user_defined() {
        let dir = ModelManager::bundled_bge_model_dir()
            .expect("bundled BGE model resource directory should exist");
        let mut model =
            ModelManager::load_user_defined_from_dir(&dir).expect("bundled BGE model should load");
        let embeddings = model.embed(vec!["测试"], None).expect("embed should work");
        assert_eq!(embeddings[0].len(), 512);
    }

    /// Full end-to-end test: download model files 鈫?load via UserDefinedEmbeddingModel.
    ///
    /// This bypasses hf-hub entirely 鈥?the model is loaded directly from downloaded bytes.
    /// Verifies the complete download-and-load pipeline works.
    #[test]
    fn test_full_download_and_load() {
        let tmp = std::env::temp_dir().join(format!("hf_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("create temp dir");

        let model_dir = tmp.join("models");
        std::fs::create_dir_all(&model_dir).expect("create model dir");

        // Files to download from qdrant ONNX repo
        let base_url = "https://hf-mirror.com/qdrant/all-MiniLM-L6-v2-onnx/resolve/main";
        let files: &[(&str, &[&str])] = &[
            ("config.json", &["config.json"]),
            ("tokenizer.json", &["tokenizer.json"]),
            ("tokenizer_config.json", &["tokenizer_config.json"]),
            ("special_tokens_map.json", &["special_tokens_map.json"]),
            ("model.onnx", &["model.onnx"]),
        ];

        let mut all_ok = true;
        for (name, sources) in files {
            let dest = model_dir.join(name);
            let mut downloaded = false;
            for src in *sources {
                let url = format!("{}/{}", base_url, src);
                println!("  downloading {} ...", name);
                match download_file_chunked(&url) {
                    Ok(data) => {
                        std::fs::write(&dest, &data).expect("write file");
                        println!("  ✓ {} ({} bytes)", name, data.len());
                        downloaded = true;
                        break;
                    }
                    Err(e) => println!("  ✗ {}: {}", src, e),
                }
            }
            if !downloaded {
                println!("  ✗ FAILED to download {}", name);
                all_ok = false;
            }
        }
        assert!(all_ok, "One or more files failed to download");

        // Load model directly from downloaded bytes
        println!("\n  Loading via UserDefinedEmbeddingModel...");
        let onnx_bytes = std::fs::read(model_dir.join("model.onnx")).expect("read model.onnx");
        let tokenizer_bytes =
            std::fs::read(model_dir.join("tokenizer.json")).expect("read tokenizer");
        let config_bytes = std::fs::read(model_dir.join("config.json")).expect("read config");
        let special_map_bytes =
            std::fs::read(model_dir.join("special_tokens_map.json")).expect("read special map");
        let tok_config_bytes =
            std::fs::read(model_dir.join("tokenizer_config.json")).expect("read tokenizer config");

        let user_model = UserDefinedEmbeddingModel::new(
            onnx_bytes,
            fastembed::TokenizerFiles {
                tokenizer_file: tokenizer_bytes,
                config_file: config_bytes,
                special_tokens_map_file: special_map_bytes,
                tokenizer_config_file: tok_config_bytes,
            },
        );

        match TextEmbedding::try_new_from_user_defined(user_model, InitOptionsUserDefined::new()) {
            Ok(model) => {
                println!("  鉁?TextEmbedding loaded via UserDefined!");
                let mut emb = model;
                let embeddings = emb.embed(vec!["娴嬭瘯"], None);
                match embeddings {
                    Ok(mut vecs) => {
                        if let Some(v) = vecs.pop() {
                            println!("  鉁?Embed test: {} dims, first={:.4}", v.len(), v[0]);
                            assert_eq!(v.len(), 384);
                        }
                    }
                    Err(e) => panic!("Embed test failed: {}", e),
                }
            }
            Err(e) => {
                panic!("UserDefined load failed: {}", e);
            }
        }

        let _ = std::fs::remove_dir_all(&tmp);
        println!("\n  鉁?Full end-to-end test PASSED");
    }
}
