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
use services::doc_generator::{GeneratedDoc, GenerateDocRequest, RecipeDocRequest, RecipeDocResult};
use services::embedding::start_download_progress_polling;
use services::hybrid_search::HybridSearchResult;
use services::ingestion::{IngestionResult, ingest_text as ingest_text_fn, ingest_file as ingest_file_fn, ingest_directory as ingest_directory_fn};
use services::llm_service::{ChatMessage, LLMConfig, RAGResponse, RAGSource, StreamChunk};
use services::memory;
use services::metadata::{ChunkMeta, DocumentMeta, KnowledgeStats};
use services::product_store::ProductMeta;
use services::research_outline::Edition;
use services::smart_completion::{SmartFillRequest, SmartFillResult};
use services::question_recommend::{RecommendRequest, RecommendedQuestion, FollowUpRequest, FollowUpResult};
use services::research_session::{ResearchSession, QARecord, SessionDetail};
use services::risk_control::{ContractScopeItem, ScopeCreepResult, ProjectHealthScore, DefenseScriptRequest, DefenseScriptResult};
use services::desensitize::DesensitizeResult;
use services::template_docx::FieldInfo;
use services::template_scanner::TemplateInfo;
use services::template_schema::TemplateSchema;
use services::vector_index::SearchResult;
use services::whisper_service::{TranscriptionResult, WhisperStatus};
use services::model_downloader;

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

/// Generate a document using a deliverable recipe (recipe-aware generation).
///
/// Full pipeline: recipe lookup → field overrides → KB search for kb-strategy fields
/// → LLM generation with recipe-specific system_prompt → template fill → product save.
#[tauri::command]
async fn generate_recipe_doc_cmd(
    state: State<'_, AppState>,
    request: RecipeDocRequest,
) -> Result<RecipeDocResult, String> {
    // Capture request data for product store before moving request
    let recipe_id = request.recipe_id.clone();
    let project = request.project_name.clone().unwrap_or_default();
    let user_field_count = request.fields.len() as i64;
    let schema_fields_json: String = serde_json::to_string(&request.schema_fields)
        .unwrap_or_else(|_| "[]".to_string());

    let result = services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await?;

    // Save product to product store for regeneration support
    let input_json = serde_json::to_string(&serde_json::json!({
        "recipe_id": recipe_id,
        "schema_fields": schema_fields_json,
        "project_name": project,
    }))
    .unwrap_or_else(|_| "{}".to_string());

    {
        let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        let _ = store.create(
            &recipe_id,
            &result.recipe_name,
            &project,
            &result.doc.output_path,
            user_field_count,
            result.doc.ai_fields.len() as i64,
            &input_json,
        );
    }

    Ok(result)
}

/// Convenience command: generate a document from research notes.
///
/// Wraps `generate_recipe_doc` with `context = Some(research_notes)`.
/// Typically used with recipe_id = "investigation_report".
#[tauri::command]
async fn generate_from_research(
    state: State<'_, AppState>,
    recipe_id: String,
    template_path: String,
    output_path: String,
    fields: std::collections::HashMap<String, String>,
    schema_fields: Option<Vec<services::template_schema::SchemaField>>,
    project_name: Option<String>,
    research_notes: String,
    project_id: Option<String>,
) -> Result<RecipeDocResult, String> {
    let request = RecipeDocRequest {
        recipe_id,
        template_path,
        output_path,
        fields,
        schema_fields: schema_fields.unwrap_or_default(),
        project_name,
        context: Some(research_notes),
        project_id,
    };
    services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// Convenience command: generate a document from meeting transcript.
///
/// Wraps `generate_recipe_doc` with `context = Some(meeting_transcript)`.
/// Typically used with recipe_id = "meeting_minutes".
#[tauri::command]
async fn generate_from_meeting(
    state: State<'_, AppState>,
    recipe_id: String,
    template_path: String,
    output_path: String,
    fields: std::collections::HashMap<String, String>,
    schema_fields: Option<Vec<services::template_schema::SchemaField>>,
    project_name: Option<String>,
    meeting_transcript: String,
    project_id: Option<String>,
) -> Result<RecipeDocResult, String> {
    let request = RecipeDocRequest {
        recipe_id,
        template_path,
        output_path,
        fields,
        schema_fields: schema_fields.unwrap_or_default(),
        project_name,
        context: Some(meeting_transcript),
        project_id,
    };
    services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
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

// ─── Phase 11: Question Recommendation Commands ───

/// Recommend relevant research questions based on the current conversation topic.
///
/// Uses hybrid_search + edition filtering + answered-question exclusion.
/// Falls back to DB-only recommendations on KB search failure.
#[tauri::command]
fn recommend_questions(
    state: State<'_, AppState>,
    request: RecommendRequest,
) -> Result<Vec<RecommendedQuestion>, String> {
    let edition = state.edition_config.current();
    services::question_recommend::recommend_questions(
        &request,
        &state.research_indexer,
        &edition,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
}

/// Generate follow-up questions based on already-answered Q&A pairs.
///
/// Searches KB for relevant context, then calls LLM to suggest 3-5 follow-up questions.
/// Graceful degradation: returns empty list on LLM or KB failure.
#[tauri::command]
async fn generate_followup_questions(
    state: State<'_, AppState>,
    request: FollowUpRequest,
) -> Result<FollowUpResult, String> {
    services::question_recommend::generate_followup_questions(
        &request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// Smart fill a research question answer using KB context + LLM.
///
/// Finds the matching question, searches KB for relevant context,
/// then calls LLM to generate an answer draft.
#[tauri::command]
async fn smart_fill_for_question(
    state: State<'_, AppState>,
    question_text: String,
    project_name: Option<String>,
) -> Result<SmartFillResult, String> {
    let edition = state.edition_config.current();
    services::question_recommend::smart_fill_for_question(
        &question_text,
        &edition,
        project_name.as_deref(),
        &state.research_indexer,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

// ─── Phase 12: Whisper Voice Recognition Commands ───

/// Load a Whisper model for voice transcription.
///
/// Downloads model from HuggingFace if not cached locally.
/// Supported sizes: "tiny" (~75MB), "base" (~142MB), "small" (~466MB).
#[tauri::command]
async fn load_whisper_model(
    state: State<'_, AppState>,
    model_size: String,
) -> Result<(), String> {
    // Use state.data_dir (initialized in AppState::new() or ensure_data_dir())
    let data_dir = &state.data_dir;
    let _model_path = model_downloader::ensure_model(data_dir, &model_size)?;

    // Load model into WhisperService
    let mut whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
    whisper.load_model(data_dir, &model_size)?;

    Ok(())
}

/// Get Whisper service status (model loaded, current model size, language).
#[tauri::command]
fn get_whisper_status(state: State<'_, AppState>) -> Result<WhisperStatus, String> {
    let whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
    Ok(whisper.status())
}

/// Start microphone recording.
///
/// Captures 16kHz mono PCM audio. Call `stop_whisper_recording` to
/// stop and get the transcription.
#[tauri::command]
fn start_whisper_recording(state: State<'_, AppState>) -> Result<(), String> {
    let capture = state.audio_capture.lock().map_err(|e| e.to_string())?;
    capture.start_recording()
}

/// Stop recording and transcribe the captured audio.
///
/// Pipeline: stop mic → PCM data → Whisper transcription →
/// Chinese post-processing → return result.
#[tauri::command]
async fn stop_whisper_recording(state: State<'_, AppState>) -> Result<TranscriptionResult, String> {
    // Step 1: Stop recording and get PCM data
    let pcm_data = {
        let capture = state.audio_capture.lock().map_err(|e| e.to_string())?;
        capture.stop_recording()?
    };

    if pcm_data.is_empty() {
        return Err("No audio data captured. Microphone may not be working.".to_string());
    }

    // Step 2: VAD — detect speech segments
    let speech_segments = services::audio_capture::AudioCapture::detect_speech_segments(
        &pcm_data, 16000, 500, 0.01,
    );

    // If no speech detected, return empty result
    if speech_segments.is_empty() {
        return Ok(TranscriptionResult {
            text: String::new(),
            segments: vec![],
            confidence: 0.0,
            processing_time_ms: 0,
        });
    }

    // Step 3: Concatenate speech segments into one buffer for Whisper
    let speech_pcm: Vec<f32> = speech_segments.iter()
        .flat_map(|(start, end)| pcm_data[*start..*end].to_vec())
        .collect();

    // Step 4: Transcribe via Whisper (offload to blocking thread since WhisperContext is not async)
    let whisper_result = {
        let whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
        if !whisper.is_model_loaded() {
            return Err("Whisper model not loaded. Call load_whisper_model first.".to_string());
        }
        // Whisper transcribe is sync and CPU-heavy
        whisper.transcribe(&speech_pcm)?
    };

    // Step 5: Chinese post-processing
    let processed_text = services::chinese_postprocess::postprocess_chinese(&whisper_result.text);

    Ok(TranscriptionResult {
        text: processed_text,
        segments: whisper_result.segments,
        confidence: whisper_result.confidence,
        processing_time_ms: whisper_result.processing_time_ms,
    })
}
// ─── Phase 13: Research Session Management ───

#[tauri::command]
fn create_research_session(
    state: State<'_, AppState>,
    title: String,
    edition: String,
    module_code: String,
    interviewee: String,
    session_date: String,
) -> Result<i64, String> {
    state.research_session_store
        .create_session(&title, &edition, &module_code, &interviewee, &session_date)
}

#[tauri::command]
fn list_research_sessions(state: State<'_, AppState>) -> Result<Vec<ResearchSession>, String> {
    state.research_session_store.list_sessions()
}

#[tauri::command]
fn get_research_session(state: State<'_, AppState>, session_id: i64) -> Result<Option<SessionDetail>, String> {
    state.research_session_store.get_session_detail(session_id)
}

#[tauri::command]
fn update_research_session(
    state: State<'_, AppState>,
    session_id: i64,
    title: String,
    interviewee: String,
    session_date: String,
    status: String,
) -> Result<(), String> {
    state.research_session_store
        .update_session(session_id, &title, &interviewee, &session_date, &status)
}

#[tauri::command]
fn delete_research_session(state: State<'_, AppState>, session_id: i64) -> Result<(), String> {
    state.research_session_store.delete_session(session_id)
}

#[tauri::command]
fn add_qa_record(
    state: State<'_, AppState>,
    session_id: i64,
    question_id: Option<i64>,
    question_text: String,
    answer_text: String,
    notes: String,
    sort_order: i32,
) -> Result<i64, String> {
    state.research_session_store
        .add_record(session_id, question_id, &question_text, &answer_text, &notes, sort_order)
}

#[tauri::command]
fn update_qa_record(
    state: State<'_, AppState>,
    record_id: i64,
    answer_text: String,
    notes: String,
) -> Result<(), String> {
    state.research_session_store.update_record(record_id, &answer_text, &notes)
}

#[tauri::command]
fn delete_qa_record(state: State<'_, AppState>, record_id: i64) -> Result<(), String> {
    state.research_session_store.delete_record(record_id)
}

#[tauri::command]
fn get_session_records(state: State<'_, AppState>, session_id: i64) -> Result<Vec<QARecord>, String> {
    state.research_session_store.get_records(session_id)
}

#[tauri::command]
fn export_session_csv(state: State<'_, AppState>, session_id: i64) -> Result<String, String> {
    state.research_session_store.export_csv(session_id)
}

#[tauri::command]
fn export_session_markdown(state: State<'_, AppState>, session_id: i64) -> Result<String, String> {
    state.research_session_store.export_markdown(session_id)
}

#[tauri::command]
fn reorder_qa_records(
    state: State<'_, AppState>,
    session_id: i64,
    record_ids: Vec<i64>,
) -> Result<(), String> {
    state.research_session_store.reorder_records(session_id, &record_ids)
}

// ─── P1: 双轨风险把控舱 ───

#[tauri::command]
fn add_scope_item(
    state: State<'_, AppState>,
    category: String,
    description: String,
    is_in_scope: bool,
    detail: String,
) -> Result<i64, String> {
    state.risk_control_store.add_scope_item(&category, &description, is_in_scope, &detail)
}

#[tauri::command]
fn list_scope_items(state: State<'_, AppState>) -> Result<Vec<ContractScopeItem>, String> {
    state.risk_control_store.list_scope_items()
}

#[tauri::command]
fn delete_scope_item(state: State<'_, AppState>, item_id: i64) -> Result<(), String> {
    state.risk_control_store.delete_scope_item(item_id)
}

#[tauri::command]
async fn check_scope_creep(
    state: State<'_, AppState>,
    requirement: String,
) -> Result<ScopeCreepResult, String> {
    state.risk_control_store.check_scope_creep(&state.llm, &requirement).await
}

#[tauri::command]
fn record_health_metric(
    state: State<'_, AppState>,
    indicator_type: String,
    value: f64,
    notes: String,
) -> Result<i64, String> {
    state.risk_control_store.record_health_metric(&indicator_type, value, &notes)
}

#[tauri::command]
fn get_project_health(state: State<'_, AppState>) -> Result<ProjectHealthScore, String> {
    state.risk_control_store.calculate_health_score()
}

#[tauri::command]
async fn generate_risk_report(
    state: State<'_, AppState>,
    context: String,
) -> Result<String, String> {
    state.risk_control_store.generate_risk_report(&state.llm, &context).await
}

#[tauri::command]
async fn generate_defense_script(
    state: State<'_, AppState>,
    request: DefenseScriptRequest,
) -> Result<DefenseScriptResult, String> {
    state.risk_control_store.generate_defense_script(&state.llm, &request).await
}

// --- P2.1: Local Desensitization ---

#[tauri::command]
fn desensitize_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<services::desensitize::DesensitizeResult, String> {
    let result = state.desensitizer.desensitize(&text);
    Ok(services::desensitize::DesensitizeResult {
        safe_text: result.safe_text,
        mapping: result.mapping,
    })
}

#[tauri::command]
fn add_sensitive_keyword(state: State<'_, AppState>, keyword: String) -> Result<(), String> {
    state.desensitizer.add_keyword(&keyword);
    Ok(())
}

#[tauri::command]
fn list_sensitive_keywords(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.desensitizer.get_keywords())
}

// --- P2.2: Blueprint Extraction ---

const BLUEPRINT_SYSTEM_PROMPT: &str = "\
你是一个金蝶ERP业务架构师。根据调研记录提炼业务蓝图。\n\
按四段结构：\n\
1.【现有流程 As-Is】具体流程步骤和角色\n\
2.【系统标准流程 To-Be】含系统路径\n\
3.【差异配置点】配置路径: 配置值\n\
4.【对应单据类型】单据名称（编码规则）\n\
禁止空话，不确定写[待确认]";

#[tauri::command]
async fn extract_blueprint(
    state: State<'_, AppState>,
    research_context: String,
) -> Result<String, String> {
    use services::llm_service::ChatMessage;
    let messages = vec![
        ChatMessage { role: "system".to_string(), content: BLUEPRINT_SYSTEM_PROMPT.to_string() },
        ChatMessage { role: "user".to_string(), content: research_context },
    ];
    let config = state.llm.get_config()?;
    state.llm.chat_completion(&messages, &config).await
}

// --- P2.3: Fit-Gap Analysis ---

const FITGAP_SYSTEM_PROMPT: &str = "\
你是一个ERP差异分析专家。分析以下需求，每项判断Fit/Gap。\n\
严格Markdown表格：|序号|需求|分类|Fit/Gap|理由|建议方案|\n\
理由必须具体到模块功能，建议必须可执行。";

#[tauri::command]
async fn analyze_fit_gap(
    state: State<'_, AppState>,
    requirements: String,
) -> Result<String, String> {
    use services::llm_service::ChatMessage;
    let messages = vec![
        ChatMessage { role: "system".to_string(), content: FITGAP_SYSTEM_PROMPT.to_string() },
        ChatMessage { role: "user".to_string(), content: requirements },
    ];
    let config = state.llm.get_config()?;
    state.llm.chat_completion(&messages, &config).await
}

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
            generate_recipe_doc_cmd,
            generate_from_research,
            generate_from_meeting,
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
            // Phase 11: Question Recommendation
            recommend_questions,
            generate_followup_questions,
            smart_fill_for_question,
            // Phase 12: Whisper Voice Recognition
            load_whisper_model,
            get_whisper_status,
            start_whisper_recording,
            stop_whisper_recording,
            // Phase 9: Research Edition Commands
            get_current_edition,
            set_edition,
            list_research_modules,
            import_research_outlines,
            // Phase 13: Research Session Management
            create_research_session,
            list_research_sessions,
            get_research_session,
            update_research_session,
            delete_research_session,
            add_qa_record,
            update_qa_record,
            delete_qa_record,
            get_session_records,
            export_session_csv,
            export_session_markdown,
            reorder_qa_records,
            // P1: 双轨风险把控舱
            add_scope_item,
            list_scope_items,
            delete_scope_item,
            check_scope_creep,
            record_health_metric,
            get_project_health,
            generate_risk_report,
            generate_defense_script,
            // P2: 蓝图提炼/Fit-Gap/脱敏
            desensitize_text,
            add_sensitive_keyword,
            list_sensitive_keywords,
            extract_blueprint,
            analyze_fit_gap,
        ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
