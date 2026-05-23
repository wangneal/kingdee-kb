//! Embedding service: text → vector conversion via fastembed-rs
//!
//! Uses bge-small-zh-v1.5 (512-dim) for Chinese text embeddings.
//! Model is auto-downloaded on first use to `~/.kingdee-kb/models/`.
//!
//! NOTE: HuggingFace is blocked in China. The model download requires:
//!   - Setting HF_ENDPOINT=https://hf-mirror.com (may not support range requests)
//!   - Or pre-downloading model files to ~/.cache/huggingface/
//!   - Or using try_new_from_user_defined() with local model files

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::path::PathBuf;

/// HuggingFace mirror list, ordered by likely speed in China.
/// `hf-hub` (used internally by fastembed) reads the `HF_ENDPOINT` env var
/// to determine which mirror to download from.
const HF_MIRRORS: &[Option<&str>] = &[
    Some("https://hf-mirror.com"),           // Official HF Chinese mirror
    None,                                      // Default (huggingface.co)
];

/// Manages the embedding model lifecycle (download, init, status)
pub struct ModelManager {
    model_dir: PathBuf,
    model: Option<TextEmbedding>,
    is_ready: bool,
}

impl ModelManager {
    /// Create a new ModelManager with the given model cache directory
    pub fn new(model_dir: PathBuf) -> Self {
        Self {
            model_dir,
            model: None,
            is_ready: false,
        }
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

        std::fs::create_dir_all(&self.model_dir)
            .map_err(|e| format!("Failed to create model directory: {}", e))?;

        // Try bge-small-zh-v1.5 (Chinese-optimized) first, fall back to all-MiniLM-L6-v2
        let model = Self::try_init_with_mirrors(EmbeddingModel::BGESmallZHV15)
            .or_else(|_| {
                eprintln!("[ModelManager] BGE model unavailable, trying default model...");
                Self::try_init_with_mirrors(EmbeddingModel::AllMiniLML6V2)
            })
            .map_err(|e| format!(
                "Failed to initialize any embedding model: {}\n\
                 Hint: The first download may take a few minutes. \
                 Try setting HF_ENDPOINT=https://hf-mirror.com in your environment, \
                 or pre-download model files to ~/.cache/huggingface/.",
                e
            ))?;

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
            match mirror {
                Some(url) => {
                    eprintln!("[ModelManager] Trying mirror {}: {}", i + 1, url);
                    std::env::set_var("HF_ENDPOINT", url);
                }
                None => {
                    eprintln!("[ModelManager] Trying mirror {}: default (huggingface.co)", i + 1);
                    // Restore original or clear to use default
                    match &original_hf_endpoint {
                        Some(val) => std::env::set_var("HF_ENDPOINT", val),
                        None => std::env::remove_var("HF_ENDPOINT"),
                    }
                }
            }

            match TextEmbedding::try_new(
                InitOptions::new(model.clone()).with_show_download_progress(true),
            ) {
                Ok(text_emb) => {
                    // Restore original HF_ENDPOINT
                    Self::restore_hf_endpoint(&original_hf_endpoint);
                    return Ok(text_emb);
                }
                Err(e) => {
                    let label = mirror.unwrap_or("default (huggingface.co)");
                    last_err = format!("{} failed: {}", label, e);
                    eprintln!("[ModelManager] Mirror {}: {} failed: {}", i + 1, label, e);

                    // Clear any partial cache from this mirror before trying the next
                    // This prevents a corrupt partial download from blocking the next mirror
                    if let Some(cache_dir) = Self::hf_cache_dir_for(&model) {
                        let _ = std::fs::remove_dir_all(&cache_dir);
                    }

                    continue;
                }
            }
        }

        // Restore original HF_ENDPOINT on failure too
        Self::restore_hf_endpoint(&original_hf_endpoint);

        Err(format!(
            "All {} mirror(s) failed for model {:?}. Last error: {}",
            HF_MIRRORS.len(),
            model,
            last_err
        ))
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
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE")) // Windows
            .ok()?;
        // Convert EmbeddingModel to HF repo ID
        // e.g., "BGESmallZHV15" → "models--BAAI--bge-small-zh-v1.5"
        // We map known model names to their HF repo IDs
        let repo_id = match model {
            EmbeddingModel::BGESmallZHV15 => "BAAI/bge-small-zh-v1.5",
            EmbeddingModel::AllMiniLML6V2 => "sentence-transformers/all-MiniLM-L6-v2",
            _ => return None,
        };
        let cache_key = format!("models--{}", repo_id.replace('/', "--"));
        Some(
            PathBuf::from(home)
                .join(".cache")
                .join("huggingface")
                .join("hub")
                .join(cache_key),
        )
    }

    /// Initialize from local model files (bypasses HuggingFace download)
    pub fn init_from_local(
        &mut self,
        onnx_path: &str,
        tokenizer_path: &str,
        config_path: &str,
    ) -> Result<(), String> {
        // NOTE: fastembed-rs v5 supports try_new_from_user_defined for local models
        // This will be used when model files are pre-bundled or pre-downloaded
        // For now, this is a placeholder for the future implementation
        let _ = (onnx_path, tokenizer_path, config_path);
        Err("Local model loading not yet implemented. Use init() with network access.".to_string())
    }

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

/// Embedding service for text vectorization
pub struct EmbeddingService {
    /// Optional: if None, the service cannot embed text
    model: Option<TextEmbedding>,
    /// Default batch size for embed_batch
    batch_size: usize,
}

impl EmbeddingService {
    /// Create a new embedding service (requires initialized model)
    pub fn new(model: TextEmbedding) -> Self {
        Self {
            model: Some(model),
            batch_size: 64,
        }
    }

    /// Create an EmbeddingService without a model (will fail on embed calls)
    pub fn empty() -> Self {
        Self {
            model: None,
            batch_size: 64,
        }
    }

    /// Embed a single text → 512-dim (or 384-dim for all-MiniLM) vector
    pub fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String> {
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

    /// Batch embed multiple texts
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
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
        self.model.is_some()
    }

    /// Inject an initialized model into this service.
    /// Called by `init_model` command after ModelManager downloads the model.
    pub fn set_model(&mut self, model: TextEmbedding) {
        self.model = Some(model);
    }

    /// Get the embedding dimension
    pub fn dimension(&self) -> usize {
        // bge-small-zh-v1.5: 512, all-MiniLM-L6-v2: 384
        // Default to 512 (bge-small-zh-v1.5 is the primary model)
        512
    }
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
