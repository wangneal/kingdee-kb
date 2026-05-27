use std::path::PathBuf;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::services::ingestion::{
    DirectoryIngestionResult, IngestionResult,
    ingest_text as ingest_text_fn,
    ingest_file as ingest_file_fn,
    ingest_directory as ingest_directory_fn,
};

/// 摄入纯文本（来自粘贴或文本框）
#[tauri::command]
pub async fn ingest_text(
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
        &state.bm25,
        Some(&app),
    )
}

/// 摄入单个文件
#[tauri::command]
pub async fn ingest_file(
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
        &state.bm25,
        Some(&app),
    )
}

/// 摄入目录中的所有支持文件
#[tauri::command]
pub async fn ingest_directory(
    state: State<'_, AppState>,
    app: AppHandle,
    dir_path: String,
    project: String,
) -> Result<DirectoryIngestionResult, String> {
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
