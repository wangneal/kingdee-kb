//! 知识编译开关的 Tauri Command

use crate::app_state::{AppState, KbRecompileFailure, KbRecompileStatus};
use crate::services::image_processor::ImageProcessor;
use crate::services::ingestion_pipeline::process_with_kb_compilation;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

pub type RecompileFailedSourceError = KbRecompileFailure;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecompileFailedSourcesResult {
    pub retried: usize,
    pub succeeded: usize,
    pub failed: Vec<RecompileFailedSourceError>,
}

#[derive(Clone)]
struct RecompileContext {
    data_dir: PathBuf,
    raw_sources: Arc<Mutex<crate::services::raw_source::RawSourceStore>>,
    ingest_cache_store: Arc<Mutex<crate::services::ingest_cache::IngestCacheStore>>,
    analysis_cache: Arc<Mutex<crate::services::analysis_cache::AnalysisCacheStore>>,
    metadata: Arc<Mutex<crate::services::metadata::MetadataStore>>,
    llm_providers: Arc<RwLock<crate::services::llm_providers::LLMProviderManager>>,
    wiki_pages: Arc<Mutex<crate::services::wiki_page::WikiPageStore>>,
    image_processor: Arc<RwLock<ImageProcessor>>,
}

impl RecompileContext {
    fn from_state(state: &AppState) -> Self {
        Self {
            data_dir: state.data_dir.clone(),
            raw_sources: state.raw_sources.clone(),
            ingest_cache_store: state.ingest_cache_store.clone(),
            analysis_cache: state.analysis_cache.clone(),
            metadata: state.metadata.clone(),
            llm_providers: state.llm_providers.clone(),
            wiki_pages: state.wiki_pages.clone(),
            image_processor: state.image_processor.clone(),
        }
    }
}

fn recompile_status_path(data_dir: &Path) -> PathBuf {
    data_dir.join("kb-recompile-status.json")
}

fn load_persisted_recompile_status(data_dir: &Path) -> Option<KbRecompileStatus> {
    let path = recompile_status_path(data_dir);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn persist_recompile_status(data_dir: &Path, status: &KbRecompileStatus) -> Result<(), String> {
    let path = recompile_status_path(data_dir);
    let json = serde_json::to_string_pretty(status)
        .map_err(|e| format!("序列化知识编译状态失败: {}", e))?;
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, json).map_err(|e| format!("写入知识编译状态临时文件失败: {}", e))?;
    std::fs::rename(&tmp_path, &path).map_err(|e| format!("保存知识编译状态失败: {}", e))
}

fn source_resume_key(identity: &str, sha256: &str) -> String {
    format!("{}::{}", identity, sha256)
}

use crate::services::docx_image_helpers::*;

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
    execute_recompile(
        RecompileContext::from_state(&state),
        project_id,
        force,
        None,
    )
    .await
}

#[tauri::command]
pub async fn start_kb_recompile(
    _app: tauri::AppHandle, // 通过 spawn_monitored 短期借用，函数体内未直接引用
    state: tauri::State<'_, AppState>,
    project_id: i64,
    force: Option<bool>,
) -> Result<KbRecompileStatus, String> {
    let force = force.unwrap_or(false);
    let status_store = state.kb_recompile_status.clone();
    let data_dir = state.data_dir.clone();
    let resume_status = load_persisted_recompile_status(&data_dir);
    let resume_source_keys = resume_status
        .as_ref()
        .filter(|status| {
            force
                && status.project_id == Some(project_id)
                && status.force
                && status.status != "completed"
        })
        .map(|status| status.completed_source_keys.clone())
        .unwrap_or_default();

    {
        let mut status = status_store
            .lock()
            .map_err(|e| format!("获取知识编译状态失败: {}", e))?;
        if status.status == "running" {
            return Ok(status.clone());
        }
        *status = KbRecompileStatus {
            status: "running".to_string(),
            project_id: Some(project_id),
            force,
            completed_source_keys: resume_source_keys.clone(),
            message: Some(if force {
                if resume_source_keys.is_empty() {
                    "强制重编译正在运行".to_string()
                } else {
                    format!(
                        "强制重编译从断点继续：已完成 {} 项",
                        resume_source_keys.len()
                    )
                }
            } else {
                "失败项重编译正在运行".to_string()
            }),
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            ..KbRecompileStatus::default()
        };
        persist_recompile_status(&data_dir, &status)?;
    }

    let context = RecompileContext::from_state(&state);
    let task_data_dir = context.data_dir.clone();
    let status_for_task = status_store.clone();
    tauri::async_runtime::spawn(async move {
        let result =
            execute_recompile(context, project_id, force, Some(status_for_task.clone())).await;
        let mut status = match status_for_task.lock() {
            Ok(status) => status,
            Err(e) => {
                tracing::warn!("更新知识编译状态失败: {}", e);
                return;
            }
        };
        match result {
            Ok(done) => {
                status.status = "completed".to_string();
                status.retried = done.retried;
                status.succeeded = done.succeeded;
                status.failed = done.failed;
                status.message = Some(if status.failed.is_empty() {
                    format!(
                        "重编译完成：成功 {}/{} 项",
                        status.succeeded, status.retried
                    )
                } else {
                    format!(
                        "重编译完成：成功 {}/{} 项，失败 {} 项",
                        status.succeeded,
                        status.retried,
                        status.failed.len()
                    )
                });
                status.finished_at = Some(chrono::Utc::now().to_rfc3339());
            }
            Err(error) => {
                status.status = "failed".to_string();
                status.message = Some(format!("重编译失败：{}", error));
                status.finished_at = Some(chrono::Utc::now().to_rfc3339());
            }
        }
        if let Err(e) = persist_recompile_status(&task_data_dir, &status) {
            tracing::warn!("保存知识编译状态失败: {}", e);
        }
    });

    get_kb_recompile_status(state).await
}

#[tauri::command]
pub async fn get_kb_recompile_status(
    state: tauri::State<'_, AppState>,
) -> Result<KbRecompileStatus, String> {
    let mut status = state
        .kb_recompile_status
        .lock()
        .map(|status| status.clone())
        .map_err(|e| format!("获取知识编译状态失败: {}", e))?;

    if status.status == "idle" {
        if let Some(mut persisted) = load_persisted_recompile_status(&state.data_dir) {
            if persisted.status == "running" {
                persisted.status = "failed".to_string();
                persisted.message =
                    Some("上次知识编译中断，可再次启动强制重编译从断点继续".to_string());
            }
            status = persisted;
        }
    }

    Ok(status)
}

async fn execute_recompile(
    context: RecompileContext,
    project_id: i64,
    force: bool,
    status_store: Option<Arc<Mutex<KbRecompileStatus>>>,
) -> Result<RecompileFailedSourcesResult, String> {
    let completed_source_keys: HashSet<String> = status_store
        .as_ref()
        .and_then(|store| {
            store
                .lock()
                .ok()
                .map(|status| status.completed_source_keys.clone())
        })
        .unwrap_or_default()
        .into_iter()
        .collect();

    let sources = {
        let store = context
            .raw_sources
            .lock()
            .map_err(|e| format!("获取 raw_sources 锁失败: {}", e))?;
        store.list_by_project(project_id)?
    };
    let total_sources = sources.len();

    let cache_keys: HashSet<(String, String)> = {
        let store = context
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
            .into_iter()
            .filter(|source| {
                !completed_source_keys
                    .contains(&source_resume_key(&source.identity, &source.sha256))
            })
            .collect()
    } else {
        sources
            .into_iter()
            .filter(|source| {
                !cache_keys.contains(&(source.identity.clone(), source.sha256.clone()))
            })
            .collect()
    };

    let skipped = if force {
        completed_source_keys.len().min(total_sources)
    } else {
        0
    };
    let retried = if force {
        total_sources
    } else {
        failed_sources.len()
    };
    if let Some(status_store) = &status_store {
        if let Ok(mut status) = status_store.lock() {
            status.retried = retried;
            status.succeeded = skipped;
            status.message = Some(if force && skipped > 0 {
                format!(
                    "强制重编译从断点继续：已完成 {} 项，剩余 {} 项",
                    skipped,
                    failed_sources.len()
                )
            } else {
                format!("重编译任务已开始，共 {} 项", retried)
            });
            if let Err(e) = persist_recompile_status(&context.data_dir, &status) {
                tracing::warn!("保存知识编译状态失败: {}", e);
            }
        }
    }
    let mut succeeded = skipped;
    let mut failed = Vec::new();

    for source in failed_sources {
        let resume_key = source_resume_key(&source.identity, &source.sha256);
        let title = source_title(&source.identity);
        let text = match extract_text_with_docx_preview_fallback(
            &context.image_processor,
            Path::new(&source.storage_path),
            "recompile",
        )
        .await
        {
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
            match context.ingest_cache_store.lock() {
                Ok(ingest) => {
                    if let Ok(Some(cache)) =
                        ingest.get_by_key(project_id, &source.identity, &source.sha256)
                    {
                        let _ = ingest.delete(cache.id);
                    }
                }
                Err(e) => tracing::warn!("获取 ingest_cache 锁失败 (force 清理): {}", e),
            }
            match context.analysis_cache.lock() {
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
            &context.metadata,
            project_id,
            &source.sha256,
            &source.identity,
        )
        .unwrap_or_else(|e| {
            tracing::warn!(
                "反查 document_id 失败 (source={}): {}，继续处理",
                source.identity,
                e
            );
            None
        });

        let compilation = process_with_kb_compilation(
            &text,
            &source.identity,
            &source.sha256,
            project_id,
            &title,
            true,
            context.analysis_cache.clone(),
            context.llm_providers.clone(),
            context.wiki_pages.clone(),
            context.ingest_cache_store.clone(),
            document_id,
            force, // force 模式传递 true，跳过 Step 0 cache 命中
        )
        .await;

        match compilation {
            Ok(result) if result.compilation_done => {
                succeeded += 1;
                if let Some(status_store) = &status_store {
                    if let Ok(mut status) = status_store.lock() {
                        if !status.completed_source_keys.contains(&resume_key) {
                            status.completed_source_keys.push(resume_key.clone());
                        }
                        if let Err(e) = persist_recompile_status(&context.data_dir, &status) {
                            tracing::warn!("保存知识编译状态失败: {}", e);
                        }
                    }
                }
            }
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

        if let Some(status_store) = &status_store {
            if let Ok(mut status) = status_store.lock() {
                status.succeeded = succeeded;
                status.failed = failed.clone();
                status.message = Some(format!(
                    "重编译进行中：已处理 {}/{} 项，成功 {} 项，失败 {} 项",
                    succeeded + failed.len(),
                    retried,
                    succeeded,
                    failed.len()
                ));
                if let Err(e) = persist_recompile_status(&context.data_dir, &status) {
                    tracing::warn!("保存知识编译状态失败: {}", e);
                }
            }
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
        .get_document_by_source_key(sha256, project_id, Some(raw_source_identity))?
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
