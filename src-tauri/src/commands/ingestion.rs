use std::path::PathBuf;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::services::ingestion::{
    ingest_directory as ingest_directory_fn, ingest_file as ingest_file_fn,
    ingest_text as ingest_text_fn, DirectoryIngestionResult, IngestionResult,
};
use crate::services::ingestion_pipeline::process_with_kb_compilation;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtractedFileText {
    pub file_path: String,
    pub title: String,
    pub text: String,
    pub char_count: usize,
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
    enable_kb_compilation: Option<bool>,
) -> Result<IngestionResult, String> {
    state.ensure_embedding_ready();

    let result = ingest_text_fn(
        &text,
        &title,
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&state.raw_sources),
        Some(&title),
        None,
        Some(&app),
    )?;

    // Step 2.5: 知识编译
    if enable_kb_compilation.unwrap_or(false) {
        let sha256 = result.sha256.clone();
        let cache_store = state.analysis_cache.clone();
        let provider_manager = state.llm_providers.clone();
        let wiki_pages = state.wiki_pages.clone();
        let ingest_cache = state.ingest_cache_store.clone();

        let _ = process_with_kb_compilation(
            &text,
            &sha256,
            &sha256,
            &project,
            &title,
            true,
            cache_store,
            provider_manager,
            wiki_pages,
            ingest_cache,
        )
        .await;
    }

    Ok(result)
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
    state.ensure_embedding_ready();

    ingest_file_fn(
        PathBuf::from(&file_path).as_path(),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&state.raw_sources),
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
    state.ensure_embedding_ready();

    ingest_directory_fn(
        PathBuf::from(&dir_path).as_path(),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&state.raw_sources),
        Some(&app),
    )
}
