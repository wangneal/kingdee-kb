#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

mod app_state;
mod services;

/// Recursively copy a directory and all its contents
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("Failed to create dir {}: {}", dst.display(), e))?;
    for entry in fs::read_dir(src).map_err(|e| format!("Failed to read dir {}: {}", src.display(), e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {} to {}: {}", src_path.display(), dst_path.display(), e))?;
        }
    }
    Ok(())
}

use app_state::AppState;
use services::bm25_service::BM25SearchResult;
use services::deliverable_recipes::DeliverableRecipe;
use services::doc_generator::{GeneratedDoc, GenerateDocRequest};
use services::embedding::start_download_progress_polling;
use services::hybrid_search::HybridSearchResult;
use services::ingestion::{IngestionResult, ingest_text as ingest_text_fn, ingest_file as ingest_file_fn, ingest_directory as ingest_directory_fn};
use services::llm_service::{ChatMessage, LLMConfig, RAGResponse, RAGSource, StreamChunk};
use services::memory;
use services::metadata::{ChunkMeta, DocumentMeta, KnowledgeStats};
use services::product_store::ProductMeta;
use services::research_outline::Edition;
use services::smart_completion::{SmartFillRequest, SmartFillResult};
use services::template_docx::FieldInfo;
use services::template_scanner::TemplateInfo;
use services::template_schema::TemplateSchema;
use services::vector_index::SearchResult;

const KEYRING_SERVICE: &str = "com.neal.kingdee-kb";

/// Tracks completion of setup tasks before closing splashscreen
struct SetupState {
    frontend_task: bool,
    backend_task: bool,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// Ensure the ~/.kingdee-kb/ data directory structure exists
fn ensure_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let data_dir = home.join(".kingdee-kb");

    let subdirs = ["knowledge", "index", "models", "bm25_index", "products"];
    for subdir in subdirs {
        fs::create_dir_all(data_dir.join(subdir))
            .map_err(|e| format!("Failed to create {}: {}", subdir, e))?;
    }

    Ok(data_dir)
}

/// Get the data directory path (available to frontend)
#[tauri::command]
fn get_data_dir() -> Result<String, String> {
    let data_dir = ensure_data_dir()?;
    Ok(data_dir.to_string_lossy().to_string())
}

/// Store an API key in the OS credential store
#[tauri::command]
fn set_api_key(service: String, key: String) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to create keyring entry: {}", e))?;
    entry
        .set_password(&key)
        .map_err(|e| format!("Failed to store API key: {}", e))?;
    Ok(())
}

/// Retrieve an API key from the OS credential store
#[tauri::command]
fn get_api_key(service: String) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to retrieve API key: {}", e)),
    }
}

/// Delete an API key from the OS credential store
#[tauri::command]
fn delete_api_key(service: String) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    entry
        .delete_credential()
        .map_err(|e| format!("Failed to delete API key: {}", e))?;
    Ok(())
}

/// Called by the frontend when React has mounted and is ready
#[tauri::command]
async fn set_complete(
    app: AppHandle,
    state: State<'_, Mutex<SetupState>>,
    task: String,
) -> Result<(), String> {
    let mut state_lock = state.lock().map_err(|e| e.to_string())?;
    match task.as_str() {
        "frontend" => state_lock.frontend_task = true,
        "backend" => state_lock.backend_task = true,
        _ => return Err(format!("invalid task: {}", task)),
    }

    if state_lock.frontend_task && state_lock.backend_task {
        if let Some(splash_window) = app.get_webview_window("splashscreen") {
            let _ = splash_window.close();
        }
        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.show();
            let _ = main_window.set_focus();
        }
    }

    Ok(())
}

// ─── Phase 2: Embedding & Vector Store Commands ───

/// Get the current model status (ready / not ready).
/// Checks EmbeddingService which holds the actual model instance.
#[tauri::command]
async fn get_model_status(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let emb = state.embedding.lock().map_err(|e| e.to_string())?;
    Ok(emb.is_ready())
}

/// Initialize the embedding model (downloads on first call).
/// After initialization, transfers the model to EmbeddingService
/// so that RAG queries can use vector search.
#[tauri::command]
async fn init_model(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    // Start progress polling so the frontend can show a download progress bar
    let download_progress = state.download_progress.clone();
    download_progress.store(0, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    start_download_progress_polling(
        &fastembed::EmbeddingModel::BGESmallZHV15,
        download_progress.clone(),
        stop,
    );

    // Step 1: Initialize model in ModelManager (may download from HuggingFace)
    let result = {
        let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
        mm.init()
    };

    // Signal the polling thread to stop
    stop_clone.store(true, Ordering::Relaxed);

    match result {
        Ok(()) => {
            download_progress.store(100, Ordering::Relaxed);
            let model = {
                let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
                mm.take_model().ok_or("Model initialized but no model returned")?
            };
            // Inject into EmbeddingService
            let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
            emb.set_model(model);
            Ok(true)
        }
        Err(e) => {
            download_progress.store(0, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// Get the current download progress of the embedding model (0–100).
#[tauri::command]
async fn get_download_progress(
    state: State<'_, AppState>,
) -> Result<u32, String> {
    Ok(state.download_progress.load(Ordering::Relaxed))
}

/// Embed a single text — returns a 512-dim vector
#[tauri::command]
async fn embed_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<Vec<f32>, String> {
    let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
    emb.embed_text(&text)
}

/// Batch embed multiple texts
#[tauri::command]
async fn embed_batch(
    state: State<'_, AppState>,
    texts: Vec<String>,
) -> Result<Vec<Vec<f32>>, String> {
    let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    emb.embed_batch(&refs)
}

/// Search for similar vectors in the HNSW index
#[tauri::command]
async fn search_similar(
    state: State<'_, AppState>,
    query: Vec<f32>,
    top_k: u32,
) -> Result<Vec<SearchResult>, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    index.search(&query, top_k as usize)
}

/// Load the vector index from disk
#[tauri::command]
async fn load_index(
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    Ok(index.len())
}

/// Get vector index statistics
#[tauri::command]
async fn get_index_stats(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    let stats = index.stats();
    serde_json::to_value(stats).map_err(|e| format!("Serialization error: {}", e))
}

/// Get knowledge base statistics (document and chunk counts)
#[tauri::command]
async fn get_knowledge_stats(
    state: State<'_, AppState>,
) -> Result<KnowledgeStats, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_stats()
}

// ─── Phase 3: Ingestion Pipeline Commands ───

/// Ingest plain text (from paste or textarea)
#[tauri::command]
async fn ingest_text(
    state: State<'_, AppState>,
    app: AppHandle,
    text: String,
    title: String,
    project: String,
) -> Result<IngestionResult, String> {
    ingest_text_fn(
        &text,
        &title,
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        Some(&app),
    )
}

/// Ingest a single file
#[tauri::command]
async fn ingest_file(
    state: State<'_, AppState>,
    app: AppHandle,
    file_path: String,
    project: String,
) -> Result<IngestionResult, String> {
    ingest_file_fn(
        PathBuf::from(&file_path).as_path(),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        Some(&app),
    )
}

/// Ingest all supported files in a directory
#[tauri::command]
async fn ingest_directory(
    state: State<'_, AppState>,
    app: AppHandle,
    dir_path: String,
    project: String,
) -> Result<Vec<IngestionResult>, String> {
    ingest_directory_fn(
        PathBuf::from(&dir_path).as_path(),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        Some(&app),
    )
}

// ─── Document Management Commands ───

/// List all documents, optionally filtered by project
#[tauri::command]
async fn list_documents(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<Vec<DocumentMeta>, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_documents(project.as_deref())
}

/// Get all chunks for a specific document
#[tauri::command]
async fn get_document_chunks(
    state: State<'_, AppState>,
    document_id: i64,
) -> Result<Vec<ChunkMeta>, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_chunks_by_document(document_id)
}

/// Delete a document and all its associated chunks
#[tauri::command]
async fn delete_document(
    state: State<'_, AppState>,
    document_id: i64,
) -> Result<(), String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.delete_document(document_id)
}

/// Get knowledge base statistics (alias for get_knowledge_stats)
#[tauri::command]
async fn get_stats(
    state: State<'_, AppState>,
) -> Result<KnowledgeStats, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_stats()
}

// ─── Phase 4: BM25 Full-Text Search Commands ───

/// Search chunks by keyword using BM25 (jieba tokenization + tantivy scoring)
#[tauri::command]
async fn bm25_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    top_k: Option<u32>,
) -> Result<Vec<BM25SearchResult>, String> {
    let bm25 = state.bm25.lock().map_err(|e| e.to_string())?;
    bm25.search(&query, project_id.as_deref(), top_k.unwrap_or(10))
}

/// Hybrid search: vector + BM25 via RRFR fusion (k=60, final top_k=5)
#[tauri::command]
async fn hybrid_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    top_k: Option<usize>,
) -> Result<Vec<HybridSearchResult>, String> {
    services::hybrid_search::hybrid_search(
        &query,
        project_id.as_deref(),
        top_k.unwrap_or(5),
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
}

// ─── Phase 6: LLM Integration & RAG Commands ───

/// Configure the LLM provider (API key, base URL, model, etc.)
#[tauri::command]
async fn set_llm_config(
    state: State<'_, AppState>,
    config: LLMConfig,
) -> Result<(), String> {
    state.llm.set_config(config)
}

/// Get current LLM configuration (API key is masked)
#[tauri::command]
async fn get_llm_config(
    state: State<'_, AppState>,
) -> Result<LLMConfig, String> {
    let mut config = state.llm.get_config()?;
    // Mask the API key for security — show first 3 and last 3 chars only for long keys
    let key_len = config.api_key.len();
    if key_len > 10 {
        config.api_key = format!(
            "{}...{}",
            &config.api_key[..3],
            &config.api_key[key_len - 3..]
        );
    } else if key_len > 0 {
        config.api_key = "****".to_string();
    }
    Ok(config)
}

/// Check if LLM is configured (has API key)
#[tauri::command]
async fn is_llm_configured(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.llm.is_configured())
}

/// Test LLM API connectivity without requiring embedding model.
///
/// Sends a minimal request to verify the API key and endpoint are valid.
/// Returns a success message or a descriptive error.
#[tauri::command]
async fn test_llm_connection(
    state: State<'_, AppState>,
) -> Result<String, String> {
    state.llm.test_connection().await
}

/// RAG query: hybrid search → context assembly → LLM streaming completion.
///
/// Returns the full response as a list of stream chunks.
/// If LLM is unavailable, returns search results in fallback mode.
#[tauri::command]
async fn rag_query(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    conversation_history: Option<Vec<ChatMessage>>,
) -> Result<RAGResponse, String> {
    let history = conversation_history.unwrap_or_default();
    state
        .llm
        .rag_query_sync(
            &query,
            project_id.as_deref(),
            history,
            &state.embedding,
            &state.vector_index,
            &state.bm25,
            &state.metadata,
        )
        .await
}

/// RAG query with streaming: returns chunks incrementally.
///
/// The frontend should listen for StreamChunk events.
#[tauri::command]
async fn rag_query_stream(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    conversation_history: Option<Vec<ChatMessage>>,
) -> Result<Vec<StreamChunk>, String> {
    let history = conversation_history.unwrap_or_default();
    state
        .llm
        .rag_query(
            &query,
            project_id.as_deref(),
            history,
            &state.embedding,
            &state.vector_index,
            &state.bm25,
            &state.metadata,
        )
        .await
}

/// Start a real-time streaming chat session via Tauri events.
///
/// Spawns a background task that emits `chat_chunk` Tauri events:
/// - `{"type": "text_delta", "content": "..."}` — text chunk
/// - `{"type": "sources", "sources": [...]}` — RAG source references
/// - `{"type": "done"}` — stream complete
/// - `{"type": "error", "message": "..."}` — error occurred
///
/// Returns immediately; the frontend should listen for `chat_chunk` events.
#[tauri::command]
async fn start_chat_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    conversation_history: Option<Vec<ChatMessage>>,
) -> Result<(), String> {
    let history = conversation_history.unwrap_or_default();

    // Clone state for background task
    let embedding = state.embedding.clone();
    let vector_index = state.vector_index.clone();
    let bm25 = state.bm25.clone();
    let metadata = state.metadata.clone();
    let llm = state.llm.clone();
    let pid = project_id.clone();
    let q = query.clone();

    // Step 1: Run hybrid_search upfront to capture sources for the UI
    let search_results = services::hybrid_search::hybrid_search(
        &q,
        pid.as_deref(),
        5,
        &*embedding,
        &*vector_index,
        &*bm25,
        &*metadata,
    )?;

    let sources: Vec<RAGSource> = search_results
        .iter()
        .map(|r| RAGSource {
            title: r.title.clone(),
            section_path: r.section_path.clone(),
            content_snippet: services::llm_service::truncate_to_tokens(&r.content, 100),
            score: r.score,
        })
        .collect();

    // Step 2: Check LLM config — fallback immediately if not configured
    if !llm.is_configured() {
        let answer = llm.fallback_response(&search_results);
        let content: String = answer.iter().map(|c| c.content.as_str()).collect();
        let sources_clone = sources.clone();
        tokio::spawn(async move {
            use tauri::Emitter;
            if !content.is_empty() {
                let _ = app.emit(
                    "chat_chunk",
                    serde_json::json!({"type": "text_delta", "content": content}),
                );
            }
            if !sources_clone.is_empty() {
                let _ = app.emit(
                    "chat_chunk",
                    serde_json::json!({"type": "sources", "sources": sources_clone}),
                );
            }
            let _ = app.emit("chat_chunk", serde_json::json!({"type": "done"}));
        });
        return Ok(());
    }

    // Step 3: Channel for streaming chunks from background task
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamChunk>(64);

    // Task A: Run RAG pipeline (with pre-computed search_results), stream to channel
    let llm_clone = llm.clone();
    tokio::spawn(async move {
        let _ = llm_clone
            .rag_query_to_sender(
                &q,
                pid.as_deref(),
                history,
                &*embedding,
                &*vector_index,
                &*bm25,
                &*metadata,
                tx,
                Some(search_results), // pass pre-computed results
            )
            .await;
    });

    // Task B: Forward chunks from channel + sources to Tauri events
    tokio::spawn(async move {
        use tauri::Emitter;
        while let Some(chunk) = rx.recv().await {
            if chunk.done {
                break;
            }
            if let Some(thinking) = &chunk.thinking {
                if !thinking.is_empty() {
                    let _ = app.emit(
                        "chat_chunk",
                        serde_json::json!({"type": "thinking", "content": thinking}),
                    );
                }
            }
            if !chunk.content.is_empty() {
                let _ = app.emit(
                    "chat_chunk",
                    serde_json::json!({"type": "text_delta", "content": chunk.content}),
                );
            }
        }
        // After streaming completes, emit sources
        if !sources.is_empty() {
            let _ = app.emit(
                "chat_chunk",
                serde_json::json!({"type": "sources", "sources": sources}),
            );
        }
        let _ = app.emit("chat_chunk", serde_json::json!({"type": "done"}));
    });

    Ok(())
}

/// Save chat memory: archive conversation + LLM extraction → ingest into KB.
///
/// Runs in background — returns immediately. Called after each chat stream completes.
#[tauri::command]
async fn save_chat_memory(
    state: State<'_, AppState>,
    conversation: Vec<ChatMessage>,
) -> Result<(), String> {
    let data_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".kingdee-kb");

    // Clone state for background task
    let llm = state.llm.clone();
    let embedding = state.embedding.clone();
    let vector_index = state.vector_index.clone();
    let metadata = state.metadata.clone();

    tokio::spawn(async move {
        memory::save_chat_memory(
            &conversation,
            &data_dir,
            &llm,
            &embedding,
            &vector_index,
            &metadata,
        )
        .await;
    });

    Ok(())
}

/// Count tokens in text (utility for frontend)
#[tauri::command]
async fn count_tokens(text: String) -> Result<u32, String> {
    Ok(services::llm_service::count_tokens(&text))
}

// ─── Phase 9: Template Parsing Engine Commands ───

/// Scan the template directory and return all templates sorted by phase.
///
/// Templates are loaded from 实施方法论V10.0交付物模板/ relative to the data directory.
#[tauri::command]
async fn scan_templates(template_dir: Option<String>) -> Result<Vec<TemplateInfo>, String> {
    let root = match template_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = dirs::home_dir().ok_or("Cannot find home directory")?;
            home.join(".kingdee-kb").join("templates")
        }
    };
    services::template_scanner::scan_templates(&root)
}

/// Extract field placeholders from a .docx or .xlsx template file.
///
/// Returns a list of `{field_name}` placeholders with their metadata.
#[tauri::command]
async fn extract_template_fields(file_path: String) -> Result<Vec<FieldInfo>, String> {
    let path = PathBuf::from(&file_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "docx" => services::template_docx::extract_docx_fields(&path),
        "xlsx" => {
            let xlsx_fields = services::template_xlsx::extract_xlsx_fields(&path)?;
            // Convert XlsxFieldInfo to FieldInfo for unified frontend API
            Ok(xlsx_fields
                .into_iter()
                .map(|f| FieldInfo {
                    name: f.name,
                    field_type: f.field_type,
                    context: f.cell_refs.join(", "),
                    count: f.count,
                })
                .collect())
        }
        _ => Err(format!("Unsupported template format: .{}", ext)),
    }
}

/// Generate a YAML schema for a template (docx or xlsx).
///
/// Parses the template, extracts fields, and returns the schema as a YAML string.
/// If `write_sidecar` is true, also writes a `.schema.yaml` file next to the template.
#[tauri::command]
async fn get_template_schema(
    template_id: String,
    template_name: String,
    file_path: String,
    phase: String,
    write_sidecar: Option<bool>,
) -> Result<TemplateSchema, String> {
    let path = PathBuf::from(&file_path);

    // Step 1: Check for pre-existing sidecar YAML
    if let Some(schema) = services::template_schema::load_schema_sidecar(&path)? {
        return Ok(schema);
    }

    // Step 2: No sidecar — extract fields from the template file
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let schema = match ext.as_str() {
        "docx" => {
            let fields = services::template_docx::extract_docx_fields(&path)?;
            services::template_schema::generate_schema_from_docx(
                &template_id,
                &template_name,
                &phase,
                &fields,
            )
        }
        "xlsx" => {
            let fields = services::template_xlsx::extract_xlsx_fields(&path)?;
            services::template_schema::generate_schema_from_xlsx(
                &template_id,
                &template_name,
                &phase,
                &fields,
            )
        }
        _ => return Err(format!("Unsupported template format: .{}", ext)),
    };

    // Optionally write sidecar YAML file for future fast loading
    if write_sidecar.unwrap_or(false) {
        services::template_schema::write_schema_sidecar(&path, &schema)?;
    }

    Ok(schema)
}

/// Generate templates.json index file listing all templates with categories.
///
/// Scans the template directory, builds a structured index grouped by phase,
/// writes it to `templates.json` in the template directory, and returns the JSON.
#[tauri::command]
async fn generate_templates_index(template_dir: Option<String>) -> Result<String, String> {
    let root = match template_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = dirs::home_dir().ok_or("Cannot find home directory")?;
            home.join(".kingdee-kb").join("templates")
        }
    };
    let output_path = root.join("templates.json");
    services::template_scanner::write_templates_json(&root, &output_path)
}

// ─── Phase 10: Document Generation Commands ───

/// Fill a template with field values (no LLM, simple replacement).
///
/// Directly replaces `{field_name}` placeholders in a .docx or .xlsx template
/// with the provided values. Returns the output path and field count.
#[tauri::command]
async fn fill_template(
    template_path: String,
    fields: std::collections::HashMap<String, String>,
    output_path: String,
) -> Result<GeneratedDoc, String> {
    services::doc_generator::fill_template(
        std::path::Path::new(&template_path),
        &fields,
        std::path::Path::new(&output_path),
    )
}

/// Generate a document by filling a template with optional LLM field generation.
///
/// Full pipeline: routes to docx/xlsx filler, calls LLM for `ai`/`llm` strategy
/// fields if schema provided, validates required fields, returns metadata.
#[tauri::command]
async fn generate_doc(
    state: State<'_, AppState>,
    request: GenerateDocRequest,
) -> Result<GeneratedDoc, String> {
    services::doc_generator::generate_document(request, &state.llm).await
}

// ─── Phase 12: Product Management Commands ───

/// List products, optionally filtered by project.
#[tauri::command]
async fn list_products(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<Vec<ProductMeta>, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.list(project.as_deref())
}

/// Get a single product by ID.
#[tauri::command]
async fn get_product(
    state: State<'_, AppState>,
    id: i64,
) -> Result<ProductMeta, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store
        .get(id)?
        .ok_or_else(|| format!("Product not found: {}", id))
}

/// Delete a product and all its versions.
#[tauri::command]
async fn delete_product(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.delete(id)
}

/// Export a product's output file to a target directory.
/// Returns the exported file path.
#[tauri::command]
async fn export_product(
    state: State<'_, AppState>,
    id: i64,
    target_dir: String,
) -> Result<String, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.export_product(id, &target_dir)
}

/// Regenerate a product with updated field values.
///
/// Re-runs document generation using the latest version's template info
/// but with the provided updated fields. Creates a new version.
#[tauri::command]
async fn regenerate_product(
    state: State<'_, AppState>,
    id: i64,
    updated_fields: std::collections::HashMap<String, String>,
) -> Result<ProductMeta, String> {
    use services::doc_generator::GenerateDocRequest;
    use std::path::PathBuf;

    // Extract all needed data from store in a block, then drop the lock
    let (product_output_path, original_input, _latest_input_data) = {
        let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;

        // Get existing product
        let product = store
            .get(id)?
            .ok_or_else(|| format!("Product not found: {}", id))?;

        // Get latest version to find the original template path
        let latest = store
            .get_latest_version(id)?
            .ok_or_else(|| format!("No versions found for product: {}", id))?;

        let original_input: serde_json::Value =
            serde_json::from_str(&latest.input_data)
                .unwrap_or_else(|_| serde_json::json!({}));

        (product.output_path.clone(), original_input, latest.input_data.clone())
    }; // store is dropped here

    let template_path = original_input
        .get("template_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Original input missing template_path".to_string())?
        .to_string();

    let schema_fields = original_input
        .get("schema_fields")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let project_name = original_input
        .get("project_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let context = original_input
        .get("context")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Generate new output path (using std::time instead of chrono)
    let timestamp = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{}", now)
    };
    let output_dir = PathBuf::from(&product_output_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = PathBuf::from(&product_output_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
    let ext = PathBuf::from(&product_output_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("docx")
        .to_string();
    let new_output_path = output_dir
        .join(format!("{}_v{}.{}", stem, timestamp, ext))
        .to_string_lossy()
        .to_string();

    // Build generation request
    let request = GenerateDocRequest {
        template_path,
        output_path: new_output_path.clone(),
        fields: updated_fields.clone(),
        schema_fields,
        project_name,
        context,
    };

    // Generate the document (no mutex held here)
    let result = services::doc_generator::generate_document(request, &state.llm).await?;

    // Save new version and update product
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    let input_json = serde_json::to_string(&serde_json::json!({
        "template_path": original_input.get("template_path"),
        "fields": updated_fields,
        "schema_fields": original_input.get("schema_fields"),
        "project_name": original_input.get("project_name"),
        "context": original_input.get("context"),
    }))
    .unwrap_or_else(|_| "{}".to_string());

    store.add_version(id, &input_json, &result.output_path)?;

    // Return updated product
    store
        .get(id)?
        .ok_or_else(|| format!("Product not found after regeneration: {}", id))
}

/// Perform backend initialization tasks
async fn setup_backend(app: AppHandle) -> Result<(), String> {
    let data_dir = ensure_data_dir()?;
    println!("Data directory initialized at: {:?}", data_dir);

    // Initialize Phase 2 services (may fail if model download blocked)
    match AppState::new(&data_dir) {
        Ok(app_state) => {
            app.manage(app_state);
            println!("Phase 2 services initialized (embedding, vector index, metadata)");
        }
        Err(e) => {
            eprintln!("WARNING: Phase 2 services failed to initialize: {}", e);
            eprintln!("The app will start in limited mode (no embedding/search/LLM).");
            app.manage(AppState::minimal(&data_dir));
        }
    }

    // Ensure template directory exists and sync built-in templates if empty
    let template_dir = data_dir.join("templates");
    if !template_dir.exists() {
        std::fs::create_dir_all(&template_dir)
            .map_err(|e| format!("Failed to create templates directory: {}", e))?;
        println!("Created templates directory at: {:?}", template_dir);
    }

    // If templates dir is empty, copy built-in templates from the app bundle
    if std::fs::read_dir(&template_dir)
        .map_err(|e| format!("Failed to read templates directory: {}", e))?
        .next()
        .is_none()
    {
        // Try the exe directory first (for production builds)
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let resource_dir = exe_dir.join("实施方法论V10.0交付物模板");
                if resource_dir.exists() {
                    match copy_dir_recursive(&resource_dir, &template_dir) {
                        Ok(_) => println!("Copied built-in templates to {:?}", template_dir),
                        Err(e) => eprintln!("Warning: Failed to copy built-in templates: {}", e),
                    }
                }
            }
        }
        // Also try the project root during development
        let dev_template_dir = std::path::PathBuf::from("../实施方法论V10.0交付物模板");
        if template_dir
            .read_dir()
            .map_err(|e| format!("Failed to read templates directory: {}", e))?
            .next()
            .is_none()
            && dev_template_dir.exists()
        {
            match copy_dir_recursive(&dev_template_dir, &template_dir) {
                Ok(_) => println!("Copied dev templates to {:?}", template_dir),
                Err(e) => eprintln!("Warning: Failed to copy dev templates: {}", e),
            }
        }
    }

    set_complete(
        app.clone(),
        app.state::<Mutex<SetupState>>(),
        "backend".to_string(),
    )
    .await
}

// ─── Phase 11: Smart Completion Commands ───

/// Smart fill: KB-assisted field filling using hybrid_search + LLM
#[tauri::command]
async fn smart_fill(state: State<'_, AppState>, request: SmartFillRequest) -> Result<SmartFillResult, String> {
    services::smart_completion::smart_fill(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// Probe missing fields: returns detailed diagnostic info for unfilled required fields
#[tauri::command]
async fn probe_missing_fields(
    state: State<'_, AppState>,
    request: GenerateDocRequest,
) -> Result<GeneratedDoc, String> {
    // Generate the document to get the latest missing fields detail
    services::doc_generator::generate_document(request, &state.llm).await
}

/// Get deliverable recipe by template_id
#[tauri::command]
fn get_deliverable_recipe(template_id: String) -> Result<DeliverableRecipe, String> {
    services::deliverable_recipes::get_recipe_by_template_id(&template_id)
        .ok_or_else(|| format!("No recipe found for template_id: {}", template_id))
}

// ─── Phase 9: Tauri Commands Registration ───

/// Get the current research edition ("enterprise" or "flagship")
#[tauri::command]
fn get_current_edition(state: State<'_, AppState>) -> Result<String, String> {
    let edition = state.edition_config.current();
    Ok(edition.as_str().to_string())
}

/// Switch the research edition
#[tauri::command]
fn set_edition(state: State<'_, AppState>, edition: String) -> Result<(), String> {
    let edition = Edition::from_str(&edition)
        .ok_or_else(|| format!("Invalid edition: {}", edition))?;
    state.edition_config.set(&edition)
}

/// List all imported research modules for the current edition
#[tauri::command]
fn list_research_modules(state: State<'_, AppState>) -> Result<Vec<(i64, String, String)>, String> {
    let edition = state.edition_config.current();
    state.research_indexer.list_outlines(&edition)
}

/// Batch import research outlines from a directory
#[tauri::command]
fn import_research_outlines(state: State<'_, AppState>, dir: String) -> Result<String, String> {
    let edition = state.edition_config.current();
    let result = state.research_indexer.import_directory(std::path::Path::new(&dir), edition)?;
    let mut summary = format!(
        "导入成功: {} 个模块, {} 个问题\n跳过: {} 个文件",
        result.imported, result.total_questions, result.skipped
    );
    if !result.errors.is_empty() {
        let error_list: Vec<&str> = result.errors.iter().take(3).map(|s| s.as_str()).collect();
        summary.push_str(&format!("\n错误 (前{}个): {}", error_list.len(), error_list.join("; ")));
    }
    if result.imported == 0 && !result.errors.is_empty() {
        return Err(format!("导入失败: {}", result.errors.join("; ")));
    }
    Ok(summary)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Initialize setup state tracking
            app.manage(Mutex::new(SetupState {
                frontend_task: false,
                backend_task: false,
            }));

            // Spawn backend initialization task
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = setup_backend(app_handle).await {
                    eprintln!("Backend setup error: {}", e);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_data_dir,
            set_api_key,
            get_api_key,
            delete_api_key,
            set_complete,
            // Phase 2: Embedding & Vector Store
            get_model_status,
            init_model,
            get_download_progress,
            embed_text,
            embed_batch,
            search_similar,
            load_index,
            get_index_stats,
            get_knowledge_stats,
            // Phase 3: Ingestion Pipeline
            ingest_text,
            ingest_file,
            ingest_directory,
            // Document Management
            list_documents,
            get_document_chunks,
            delete_document,
            get_stats,
            // Phase 4: BM25 Search
            bm25_search,
            hybrid_search,
            // Phase 6: LLM Integration
            set_llm_config,
            get_llm_config,
            is_llm_configured,
            test_llm_connection,
            rag_query,
            rag_query_stream,
            start_chat_stream,
            save_chat_memory,
            count_tokens,
            // Phase 9: Template Engine
            scan_templates,
            extract_template_fields,
            get_template_schema,
            generate_templates_index,
            // Phase 10: Document Generation
            fill_template,
            generate_doc,
            // Phase 12: Product Management
            list_products,
            get_product,
            delete_product,
            export_product,
            regenerate_product,
            // Phase 11: Smart Completion
            smart_fill,
            probe_missing_fields,
            get_deliverable_recipe,
            // Phase 9: Research Edition Commands
            get_current_edition,
            set_edition,
            list_research_modules,
            import_research_outlines,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
