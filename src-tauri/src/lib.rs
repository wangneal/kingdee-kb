use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::async_runtime::spawn;
use tauri::{AppHandle, Manager, State};

mod app_state;
mod services;

use app_state::AppState;
use services::bm25_service::BM25SearchResult;
use services::hybrid_search::HybridSearchResult;
use services::vector_index::SearchResult;
use services::metadata::{ChunkMeta, DocumentMeta, KnowledgeStats};
use services::ingestion::{IngestionResult, ingest_text as ingest_text_fn, ingest_file as ingest_file_fn, ingest_directory as ingest_directory_fn};
use services::llm_service::{ChatMessage, LLMConfig, RAGResponse, StreamChunk};
use services::doc_generator::{GeneratedDoc, GenerateDocRequest};
use services::product_store::ProductMeta;
use services::smart_completion::{SmartFillRequest, SmartFillResult};
use services::deliverable_recipes::DeliverableRecipe;
use services::template_docx::FieldInfo;
use services::template_scanner::TemplateInfo;
use services::template_schema::TemplateSchema;

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
        }
    }

    Ok(())
}

// ─── Phase 2: Embedding & Vector Store Commands ───

/// Get the current model status (ready / not ready)
#[tauri::command]
async fn get_model_status(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let mm = state.model_manager.lock().map_err(|e| e.to_string())?;
    Ok(mm.is_ready())
}

/// Initialize the embedding model (downloads on first call)
#[tauri::command]
async fn init_model(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
    mm.init()?;
    Ok(mm.is_ready())
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
    // Mask the API key for security
    if config.api_key.len() > 8 {
        config.api_key = format!(
            "{}...{}",
            &config.api_key[..4],
            &config.api_key[config.api_key.len() - 4..]
        );
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

    // Optionally write sidecar YAML file
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
            // Log the error but don't block the app from starting
            eprintln!("WARNING: Phase 2 services failed to initialize: {}", e);
            eprintln!("The app will start in limited mode (no embedding/search/LLM).");
            // Manage a minimal AppState so Tauri commands don't crash
            app.manage(AppState::minimal(&data_dir));
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_keyring_store::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(SetupState {
            frontend_task: false,
            backend_task: false,
        }))
        .setup(|app| {
            spawn(setup_backend(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Phase 1 commands
            greet,
            set_complete,
            get_data_dir,
            set_api_key,
            get_api_key,
            delete_api_key,
            // Phase 2 commands
            get_model_status,
            init_model,
            embed_text,
            embed_batch,
            search_similar,
            load_index,
            get_index_stats,
            get_knowledge_stats,
            // Phase 3 commands
            ingest_text,
            ingest_file,
            ingest_directory,
            // Document management
            list_documents,
            get_document_chunks,
            delete_document,
            get_stats,
            // Phase 4 commands
            bm25_search,
            // Phase 5 commands
            hybrid_search,
            // Phase 6 commands
            set_llm_config,
            get_llm_config,
            is_llm_configured,
            rag_query,
            rag_query_stream,
            count_tokens,
            // Phase 9 commands
            scan_templates,
            extract_template_fields,
            get_template_schema,
            generate_templates_index,
            // Phase 10 commands
            fill_template,
            generate_doc,
            // Phase 12 commands
            list_products,
            get_product,
            delete_product,
            export_product,
            regenerate_product,
            // Phase 11 commands
            smart_fill,
            probe_missing_fields,
            get_deliverable_recipe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
