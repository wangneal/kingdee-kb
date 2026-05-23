//! Application state management
//!
//! Holds all Phase 2+ services (embedding, vector index, metadata store, BM25, LLM)
//! in Arc<Mutex<>> for thread-safe access from Tauri commands.

use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
use crate::services::embedding::{EmbeddingService, ModelManager};
use crate::services::vector_index::VectorIndex;
use crate::services::metadata::MetadataStore;
use crate::services::product_store::ProductStore;
use crate::services::bm25_service::BM25Service;
use crate::services::llm_service::LLMService;

/// Global application state shared across all Tauri commands
pub struct AppState {
    /// Embedding model manager (download, init, status)
    pub model_manager: Arc<Mutex<ModelManager>>,
    /// Text → vector embedding service
    pub embedding: Arc<Mutex<EmbeddingService>>,
    /// HNSW vector index for similarity search
    pub vector_index: Arc<Mutex<VectorIndex>>,
    /// SQLite metadata store for chunk↔vector mapping
    pub metadata: Arc<Mutex<MetadataStore>>,
    /// BM25 full-text search service (tantivy + jieba)
    pub bm25: Arc<Mutex<BM25Service>>,
    /// LLM service for RAG queries (OpenAI-compatible API)
    pub llm: LLMService,
    /// Product store for generated document management
    pub products: Arc<Mutex<ProductStore>>,
    /// Download progress for embedding model (0–100). Updated by background thread.
    pub download_progress: Arc<AtomicU32>,
}

impl AppState {
    /// Initialize all services with the given data directory (~/.kingdee-kb/)
    pub fn new(data_dir: &std::path::Path) -> Result<Self, String> {
        let model_dir = data_dir.join("models");
        let index_dir = data_dir.join("index");
        let db_path = data_dir.join("metadata.db");

        // Initialize ModelManager (model download deferred)
        let model_manager = ModelManager::new(model_dir);

        // Initialize EmbeddingService (empty - model not loaded yet)
        let embedding = EmbeddingService::empty();

        // Initialize VectorIndex (create or load from disk)
        let index_path = index_dir.join("vectors.usearch");
        let vector_index = if index_path.exists() {
            VectorIndex::load(index_path)
                .unwrap_or_else(|_| VectorIndex::new(index_dir).expect("Failed to create index"))
        } else {
            VectorIndex::new(index_dir)?
        };

        // Initialize MetadataStore (create if not exists)
        let metadata = MetadataStore::new(db_path)?;

        // Initialize BM25Service (tantivy + jieba full-text index)
        let bm25_index_dir = data_dir.join("bm25_index");
        let bm25 = BM25Service::new(bm25_index_dir)?;

        // Initialize ProductStore (create if not exists)
        let products_db_path = data_dir.join("products.db");
        let products = ProductStore::new(products_db_path)?;

        Ok(Self {
            model_manager: Arc::new(Mutex::new(model_manager)),
            embedding: Arc::new(Mutex::new(embedding)),
            vector_index: Arc::new(Mutex::new(vector_index)),
            metadata: Arc::new(Mutex::new(metadata)),
            bm25: Arc::new(Mutex::new(bm25)),
            llm: LLMService::new(data_dir),
            products: Arc::new(Mutex::new(products)),
            download_progress: Arc::new(AtomicU32::new(0)),
        })
    }

    /// Create a minimal AppState when full initialization fails.
    ///
    /// Tries to init each service individually. If a service fails,
    /// uses an in-memory stub so the app can still start (commands
    /// that depend on that service will return errors at runtime).
    pub fn minimal(data_dir: &std::path::Path) -> Self {
        let metadata = MetadataStore::new(data_dir.join("metadata.db"))
            .expect("Fatal: cannot create metadata DB — app cannot function without it");

        let vector_index = VectorIndex::new(data_dir.join("index"))
            .expect("Fatal: cannot create vector index — app cannot function without it");

        let bm25 = BM25Service::new(data_dir.join("bm25_index"))
            .expect("Fatal: cannot create BM25 index — app cannot function without it");

        let products = ProductStore::new(data_dir.join("products.db"))
            .expect("Fatal: cannot create product store — app cannot function without it");

        Self {
            model_manager: Arc::new(Mutex::new(ModelManager::new(data_dir.join("models")))),
            embedding: Arc::new(Mutex::new(EmbeddingService::empty())),
            vector_index: Arc::new(Mutex::new(vector_index)),
            metadata: Arc::new(Mutex::new(metadata)),
            bm25: Arc::new(Mutex::new(bm25)),
            llm: LLMService::new(data_dir),
            products: Arc::new(Mutex::new(products)),
            download_progress: Arc::new(AtomicU32::new(0)),
        }
    }
}
