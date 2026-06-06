//! 知识编译开关的 Tauri Command

use crate::app_state::AppState;
use crate::services::ingestion_pipeline::{process_with_kb_compilation, KbCompilationResult};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

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
    force: Option<bool>,
) -> Result<RecompileFailedSourcesResult, String> {
    let force = force.unwrap_or(false);
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

    // force=true：重编译全部；force=false：仅重编译 cache 缺失的源
    let failed_sources: Vec<_> = if force {
        sources
    } else {
        sources
            .into_iter()
            .filter(|source| {
                !cache_keys.contains(&(source.identity.clone(), source.sha256.clone()))
            })
            .collect()
    };

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

        // force 模式下：先清掉 cache，让后续 process_with_kb_compilation 不会命中旧 cache
        if force {
            match state.ingest_cache_store.lock() {
                Ok(ingest) => {
                    if let Ok(Some(cache)) =
                        ingest.get_by_key(project_id, &source.identity, &source.sha256)
                    {
                        let _ = ingest.delete(cache.id);
                    }
                }
                Err(e) => tracing::warn!("获取 ingest_cache 锁失败 (force 清理): {}", e),
            }
            match state.analysis_cache.lock() {
                Ok(analysis) => {
                    if let Ok(Some(cache)) =
                        analysis.get_by_key(project_id, &source.identity, &source.sha256)
                    {
                        let _ = analysis.delete(cache.id);
                    }
                }
                Err(e) => tracing::warn!("获取 analysis_cache 锁失败 (force 清理): {}", e),
            }
        }

        // 关键：反查 document_id，让 process_with_kb_compilation 写入正确的 sources.document_id
        // （之前固定传 None，导致新 wiki 页面 sources.document_id=null，触发原 bug）
        // 反查失败时不终止整个批量流程，回退到 None 继续处理其他源
        let document_id = lookup_document_id(
            &state.metadata,
            project_id,
            &source.sha256,
            &source.identity,
        )
        .unwrap_or_else(|e| {
            tracing::warn!("反查 document_id 失败 (source={}): {}，继续处理", source.identity, e);
            None
        });

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
            document_id,
            force, // force 模式传递 true，跳过 Step 0 cache 命中
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

/// 按 (project_id, sha256, raw_source_identity) 反查 documents 表的 document_id
/// 用于"删 wiki 后强制重编译"场景：原 document 记录被删，但 raw_sources 还在
/// 关联键：documents.sha256 = source.sha256 AND documents.project_id = source.project_id
///        AND documents.raw_source_identity = source.identity
pub(crate) fn lookup_document_id(
    metadata: &Arc<Mutex<crate::services::metadata::MetadataStore>>,
    project_id: i64,
    sha256: &str,
    raw_source_identity: &str,
) -> Result<Option<i64>, String> {
    let meta = metadata
        .lock()
        .map_err(|e| format!("获取 metadata 锁失败: {}", e))?;
    Ok(meta
        .get_document_by_sha256(sha256)?
        .filter(|doc| {
            doc.project_id == project_id
                && doc.raw_source_identity.as_deref() == Some(raw_source_identity)
        })
        .map(|doc| doc.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── source_title 单元测试 ───
    // 用于从 raw_sources.identity（带扩展名）提取页面 title

    #[test]
    fn source_title_strips_md_extension() {
        assert_eq!(source_title("需求跟踪矩阵.md"), "需求跟踪矩阵");
    }

    #[test]
    fn source_title_strips_xlsx_extension() {
        assert_eq!(
            source_title("05需求跟踪矩阵_模板（for V10.0）.xlsx"),
            "05需求跟踪矩阵_模板（for V10.0）"
        );
    }

    #[test]
    fn source_title_strips_docx_extension() {
        assert_eq!(
            source_title("开发需求设计确认单.docx"),
            "开发需求设计确认单"
        );
    }

    #[test]
    fn source_title_strips_path_prefix() {
        // identity 可能含路径前缀
        assert_eq!(source_title("C:/Users/Neal/原始资料/需求.docx"), "需求");
    }

    #[test]
    fn source_title_no_extension_returns_as_is() {
        assert_eq!(source_title("无扩展名"), "无扩展名");
    }

    #[test]
    fn source_title_empty_string_returns_input() {
        // 空字符串会触发 unwrap_or(identity) 兜底
        assert_eq!(source_title(""), "");
    }
}

/// 强制重编译指定的源（用于"删 wiki 后原地重生成"场景）
///
/// 流程：
/// 1. 按 source_id 查出 raw_source，校验 project_id
/// 2. 重新读取源文件提取文本
/// 3. 强制清除该源的 ingest_cache 和 analysis_cache
/// 4. 调用 process_with_kb_compilation，force_recompile=true 跳过 cache 检查
#[tauri::command]
pub async fn force_recompile_kb_source(
    state: tauri::State<'_, AppState>,
    project_id: i64,
    source_id: i64,
) -> Result<KbCompilationResult, String> {
    // 1. 读取源记录
    let source = {
        let store = state
            .raw_sources
            .lock()
            .map_err(|e| format!("获取 raw_sources 锁失败: {}", e))?;
        store.get_by_id(source_id)?
    };

    // 2. 校验 project_id 一致
    if source.project_id != project_id {
        return Err(format!(
            "源项目不匹配: source.project_id={}, 传入 project_id={}",
            source.project_id, project_id
        ));
    }

    let title = source_title(&source.identity);

    // 3. 反查 document_id（确保新建 wiki 页面 sources 含真实 document_id，避免 null）
    //    关联键：documents.sha256 = source.sha256 AND documents.project_id = source.project_id
    //    AND documents.raw_source_identity = source.identity
    let document_id: Option<i64> = lookup_document_id(
        &state.metadata,
        project_id,
        &source.sha256,
        &source.identity,
    )?;

    // 4. 重新读取并提取文本
    let text = crate::services::file_extractor::extract_text(Path::new(&source.storage_path))
        .map_err(|e| format!("读取原始资料失败: {}", e))?;

    // 5. 强制清除 ingest_cache 和 analysis_cache
    {
        let ingest = state
            .ingest_cache_store
            .lock()
            .map_err(|e| format!("获取 ingest_cache 锁失败: {}", e))?;
        if let Ok(Some(cache)) = ingest.get_by_key(project_id, &source.identity, &source.sha256) {
            let _ = ingest.delete(cache.id);
            tracing::info!(
                "force_recompile 已清 ingest_cache: source={}, sha256={}",
                source.identity,
                source.sha256
            );
        }
    }
    {
        let analysis = state
            .analysis_cache
            .lock()
            .map_err(|e| format!("获取 analysis_cache 锁失败: {}", e))?;
        if let Ok(Some(cache)) = analysis.get_by_key(project_id, &source.identity, &source.sha256) {
            let _ = analysis.delete(cache.id);
            tracing::info!(
                "force_recompile 已清 analysis_cache: source={}",
                source.identity
            );
        }
    }

    // 6. 走完整流程（force_recompile=true 跳过 Step 0 的 cache 检查）
    process_with_kb_compilation(
        &text,
        &source.identity,
        &source.sha256,
        project_id,
        &title,
        true, // enable_kb_compilation
        state.analysis_cache.clone(),
        state.llm_providers.clone(),
        state.wiki_pages.clone(),
        state.ingest_cache_store.clone(),
        document_id, // 传入反查到的 document_id（issue 2 修复）
        true,        // force_recompile：跳过 cache 命中检查
    )
    .await
}
