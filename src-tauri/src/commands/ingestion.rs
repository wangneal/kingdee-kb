use std::path::PathBuf;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::services::ingestion::{
    ingest_directory as ingest_directory_fn, ingest_file as ingest_file_fn,
    ingest_text as ingest_text_fn, DirectoryIngestionResult, IngestionResult,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtractedFileText {
    pub file_path: String,
    pub title: String,
    pub text: String,
    pub char_count: usize,
}

/// 确保 embedding 模型已加载（懒加载）。
/// 复用 search_llm 中的逻辑。
fn ensure_embedding_ready(
    embedding: &std::sync::Mutex<crate::services::embedding::EmbeddingService>,
    model_manager: &std::sync::Mutex<crate::services::embedding::ModelManager>,
) {
    let emb = embedding.lock().unwrap();
    if emb.is_ready() {
        return;
    }
    drop(emb);

    let mut mm = match model_manager.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if !mm.is_ready() {
        if let Err(e) = mm.init() {
            eprintln!("[LazyLoad] Model init failed: {}", e);
            return;
        }
    }
    if let Some(model) = mm.take_model() {
        let mut emb = embedding.lock().unwrap();
        emb.set_model(model);
        println!("[LazyLoad] Embedding model loaded on first use!");
    }
}

/// 摄入纯文本（来自粘贴或文本框）
///
/// 首次调用时会自动加载 embedding 模型（懒加载）。
#[tauri::command]
pub async fn ingest_text(
    state: State<'_, AppState>,
    app: AppHandle,
    text: String,
    title: String,
    project: String,
) -> Result<IngestionResult, String> {
    ensure_embedding_ready(&state.embedding, &state.model_manager);

    ingest_text_fn(
        &text,
        &title,
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&app),
    )
}

/// 摄入单个文件
///
/// 首次调用时会自动加载 embedding 模型（懒加载）。
#[tauri::command]
pub async fn ingest_file(
    state: State<'_, AppState>,
    app: AppHandle,
    file_path: String,
    project: String,
) -> Result<IngestionResult, String> {
    ensure_embedding_ready(&state.embedding, &state.model_manager);

    ingest_file_fn(
        PathBuf::from(&file_path).as_path(),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&app),
    )
}

#[tauri::command]
pub async fn extract_file_text(file_path: String) -> Result<ExtractedFileText, String> {
    let path = PathBuf::from(&file_path);
    let text = crate::services::file_extractor::extract_text(&path)?;
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("attachment")
        .to_string();
    let char_count = text.chars().count();

    Ok(ExtractedFileText {
        file_path,
        title,
        text,
        char_count,
    })
}

/// 摄入目录中的所有支持文件
///
/// 首次调用时会自动加载 embedding 模型（懒加载）。
#[tauri::command]
pub async fn ingest_directory(
    state: State<'_, AppState>,
    app: AppHandle,
    dir_path: String,
    project: String,
) -> Result<DirectoryIngestionResult, String> {
    ensure_embedding_ready(&state.embedding, &state.model_manager);

    ingest_directory_fn(
        PathBuf::from(&dir_path).as_path(),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&app),
    )
}
