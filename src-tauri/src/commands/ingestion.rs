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

/// 读取 KB 编译开关：优先使用调用参数，否则读取持久化配置
fn resolve_kb_compilation(state: &State<'_, AppState>, param: Option<bool>) -> bool {
    if let Some(v) = param {
        return v;
    }
    // 从持久化配置读取
    state
        .metadata
        .lock()
        .ok()
        .and_then(|m| m.get_kb_compilation_enabled().ok())
        .unwrap_or(false)
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
    project_id: i64,
    enable_kb_compilation: Option<bool>,
) -> Result<IngestionResult, String> {
    state.ensure_embedding_ready();

    // 纯文本导入 identity 使用 title
    let source_identity = title.clone();

    let mut result = ingest_text_fn(
        &text,
        &title,
        project_id,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&state.raw_sources),
        Some(&source_identity),
        None,
        Some(&app),
        Some(&state.data_dir),
    )?;

    // 知识编译
    if resolve_kb_compilation(&state, enable_kb_compilation) {
        let cache_store = state.analysis_cache.clone();
        let provider_manager = state.llm_providers.clone();
        let wiki_pages = state.wiki_pages.clone();
        let ingest_cache = state.ingest_cache_store.clone();

        // source_identity 关联 raw_sources.identity，而非 sha256
        match process_with_kb_compilation(
            &text,
            &source_identity,
            &result.sha256,
            project_id,
            &title,
            true,
            cache_store,
            provider_manager,
            wiki_pages,
            ingest_cache,
        )
        .await
        {
            Ok(compilation) => {
                result.kb_analysis_engine = Some(compilation.engine);
            }
            Err(e) => {
                tracing::warn!("KB 编译失败（文本导入）: {}", e);
                // 导入本身已成功，将编译失败记录在结构化字段中
                result.kb_compilation_error = Some(format!("{}", e));
            }
        }
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
    project_id: i64,
    enable_kb_compilation: Option<bool>,
) -> Result<IngestionResult, String> {
    state.ensure_embedding_ready();

    let mut result = ingest_file_fn(
        PathBuf::from(&file_path).as_path(),
        project_id,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&state.raw_sources),
        Some(&app),
        Some(&state.data_dir),
    )?;

    // 知识编译
    if resolve_kb_compilation(&state, enable_kb_compilation) {
        let text = crate::services::file_extractor::extract_text(&PathBuf::from(&file_path))
            .map_err(|e| format!("读取文件内容失败: {}", e))?;
        let sha256 = result.sha256.clone();
        let title = result.title.clone();
        // source_identity 使用文件名（与 raw_sources.identity 一致）
        let source_identity = PathBuf::from(&file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let cache_store = state.analysis_cache.clone();
        let provider_manager = state.llm_providers.clone();
        let wiki_pages = state.wiki_pages.clone();
        let ingest_cache = state.ingest_cache_store.clone();

        match process_with_kb_compilation(
            &text,
            &source_identity,
            &sha256,
            project_id,
            &title,
            true,
            cache_store,
            provider_manager,
            wiki_pages,
            ingest_cache,
        )
        .await
        {
            Ok(compilation) => {
                result.kb_analysis_engine = Some(compilation.engine);
            }
            Err(e) => {
                tracing::warn!("KB 编译失败（文件导入）: {}", e);
                // 导入本身已成功，将编译失败记录在结构化字段中
                result.kb_compilation_error = Some(format!("{}", e));
            }
        }
    }

    Ok(result)
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
    project_id: i64,
    enable_kb_compilation: Option<bool>,
) -> Result<DirectoryIngestionResult, String> {
    state.ensure_embedding_ready();

    let mut result = ingest_directory_fn(
        PathBuf::from(&dir_path).as_path(),
        project_id,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&state.raw_sources),
        Some(&app),
        Some(&state.data_dir),
    )?;

    // 知识编译（对每个成功导入的文件）
    if resolve_kb_compilation(&state, enable_kb_compilation) {
        let cache_store = state.analysis_cache.clone();
        let provider_manager = state.llm_providers.clone();
        let wiki_pages = state.wiki_pages.clone();
        let ingest_cache = state.ingest_cache_store.clone();

        for imported in &mut result.imported {
            if let Some(ref sp) = imported.source_path {
                match crate::services::file_extractor::extract_text(&PathBuf::from(sp)) {
                    Ok(text) => {
                        let sha256 = imported.sha256.clone();
                        let title = imported.title.clone();
                        // source_identity 使用文件名（与 raw_sources.identity 一致）
                        let source_identity = PathBuf::from(sp)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        match process_with_kb_compilation(
                            &text,
                            &source_identity,
                            &sha256,
                            project_id,
                            &title,
                            true,
                            cache_store.clone(),
                            provider_manager.clone(),
                            wiki_pages.clone(),
                            ingest_cache.clone(),
                        )
                        .await
                        {
                            Ok(compilation) => {
                                imported.kb_analysis_engine = Some(compilation.engine);
                            }
                            Err(e) => {
                                tracing::warn!("KB 编译失败: {}: {}", title, e);
                                imported.kb_compilation_error = Some(format!("{}", e));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("KB 编译读取文件失败: {} — {}", sp, e);
                        imported.kb_compilation_error = Some(format!("读取文件失败: {}", e));
                    }
                }
            }
        }
    }

    Ok(result)
}
