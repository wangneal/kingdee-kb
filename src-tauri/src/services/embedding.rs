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

    /// Initialize the embedding model (downloads on first use)
    pub fn init(&mut self) -> Result<(), String> {
        if self.is_ready {
            return Ok(());
        }

        // Ensure model directory exists
        std::fs::create_dir_all(&self.model_dir)
            .map_err(|e| format!("Failed to create model directory: {}", e))?;

        // Try to initialize with bge-small-zh-v1.5
        // Falls back to all-MiniLM-L6-v2 if BGE model unavailable
        let model = Self::try_init_model(EmbeddingModel::BGESmallZHV15)
            .or_else(|_| {
                eprintln!("[ModelManager] BGE model unavailable, trying default model...");
                Self::try_init_model(EmbeddingModel::AllMiniLML6V2)
            })
            .map_err(|e| format!(
                "Failed to initialize any embedding model: {}\n\
                 Hint: Set HF_ENDPOINT=https://hf-mirror.com or pre-download model files.",
                e
            ))?;

        self.model = Some(model);
        self.is_ready = true;

        Ok(())
    }

    fn try_init_model(model: EmbeddingModel) -> Result<TextEmbedding, String> {
        let model_name = format!("{:?}", model);
        TextEmbedding::try_new(
            InitOptions::new(model).with_show_download_progress(true),
        )
        .map_err(|e| format!("Failed to init {}: {}", model_name, e))
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
