//! 知识编译开关的 Tauri Command

use crate::app_state::AppState;
use crate::services::ingestion_pipeline::process_with_kb_compilation;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecompileFailedSourceError {
    pub source_id: i64,
    pub title: String,
    pub error: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecompileFailedSourcesResult {
    pub retried: usize,
    pub succeeded: usize,
    pub failed: Vec<RecompileFailedSourceError>,
}

/// 获取知识编译开关状态
#[tauri::command]
pub async fn get_kb_compilation_enabled(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let store = state
        .metadata
        .lock()
        .map_err(|e| format!("获取 metadata 锁失败: {}", e))?;
    store.get_kb_compilation_enabled()
}

/// 设置知识编译开关状态
#[tauri::command]
pub async fn set_kb_compilation_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let store = state
        .metadata
        .lock()
        .map_err(|e| format!("获取 metadata 锁失败: {}", e))?;
    store.set_kb_compilation_enabled(enabled)
}

#[tauri::command]
pub async fn recompile_failed_kb_sources(
    state: tauri::State<'_, AppState>,
    project_id: i64,
) -> Result<RecompileFailedSourcesResult, String> {
    let sources = {
        let store = state
            .raw_sources
            .lock()
            .map_err(|e| format!("获取 raw_sources 锁失败: {}", e))?;
        store.list_by_project(project_id)?
    };

    let cache_keys: HashSet<(String, String)> = {
        let store = state
            .ingest_cache_store
            .lock()
            .map_err(|e| format!("获取 ingest_cache 锁失败: {}", e))?;
        store
            .list_by_project(project_id)?
            .into_iter()
            .map(|cache| (cache.source_identity, cache.sha256))
            .collect()
    };

    let failed_sources: Vec<_> = sources
        .into_iter()
        .filter(|source| !cache_keys.contains(&(source.identity.clone(), source.sha256.clone())))
        .collect();

    let retried = failed_sources.len();
    let mut succeeded = 0;
    let mut failed = Vec::new();

    for source in failed_sources {
        let title = source_title(&source.identity);
        let text =
            match crate::services::file_extractor::extract_text(Path::new(&source.storage_path)) {
                Ok(text) => text,
                Err(error) => {
                    failed.push(RecompileFailedSourceError {
                        source_id: source.id,
                        title,
                        error: format!("读取原始资料失败: {}", error),
                    });
                    continue;
                }
            };

        let compilation = process_with_kb_compilation(
            &text,
            &source.identity,
            &source.sha256,
            project_id,
            &title,
            true,
            state.analysis_cache.clone(),
            state.llm_providers.clone(),
            state.wiki_pages.clone(),
            state.ingest_cache_store.clone(),
        )
        .await;

        match compilation {
            Ok(result) if result.compilation_done => succeeded += 1,
            Ok(_) => failed.push(RecompileFailedSourceError {
                source_id: source.id,
                title,
                error: "知识编译未完成".to_string(),
            }),
            Err(error) => failed.push(RecompileFailedSourceError {
                source_id: source.id,
                title,
                error,
            }),
        }
    }

    Ok(RecompileFailedSourcesResult {
        retried,
        succeeded,
        failed,
    })
}

fn source_title(identity: &str) -> String {
    Path::new(identity)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or(identity)
        .to_string()
}
