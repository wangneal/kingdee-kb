//! 知识编译开关的 Tauri Command

use crate::app_state::{AppState, KbRecompileFailure, KbRecompileStatus};
use crate::services::image_processor::ImageProcessor;
use crate::services::ingestion_pipeline::{process_with_kb_compilation, KbCompilationResult};
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

fn is_docx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("docx"))
        .unwrap_or(false)
}

fn create_docx_preview_temp_dir(file_path: &Path) -> Result<PathBuf, String> {
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("docx");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "kingdee-kb-recompile-docx-preview-{}-{}-{}",
        std::process::id(),
        nonce,
        stem
    ));
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建 DOCX 预览图临时目录失败 {:?}: {}", dir.display(), e))?;
    Ok(dir)
}

fn clone_image_processor(
    image_processor: &Arc<RwLock<ImageProcessor>>,
) -> Result<ImageProcessor, String> {
    let guard = image_processor
        .read()
        .map_err(|e| format!("获取图片处理器失败: {}", e))?;
    Ok(guard.clone_configured())
}

async fn extract_image_text_with_processor(
    file_path: &str,
    processor: ImageProcessor,
) -> Result<String, String> {
    let result = processor
        .process_image(file_path)
        .await
        .map_err(|e| format!("图片处理失败: {}", e))?;
    if result.text.trim().is_empty() {
        return Err("图片中未识别到任何文本".to_string());
    }
    Ok(result.text)
}

async fn extract_docx_preview_text(
    context: &RecompileContext,
    file_path: &Path,
) -> Result<String, String> {
    let can_process = {
        let guard = context
            .image_processor
            .read()
            .map_err(|e| format!("获取图片处理器失败: {}", e))?;
        guard.can_process_images()
    };
    if !can_process {
        return Err("DOCX 内嵌 Visio 无法直接提取文字，且未配置 OCR 或多模态视觉模型".to_string());
    }

    let temp_dir = create_docx_preview_temp_dir(file_path)?;
    let preview_paths =
        crate::services::file_extractor::extract_docx_preview_images(file_path, &temp_dir)?;
    let mut sections = Vec::new();
    let mut errors = Vec::new();

    for preview_path in preview_paths {
        let preview_path_str = preview_path.to_string_lossy().to_string();
        let processor = clone_image_processor(&context.image_processor)?;
        match extract_image_text_with_processor(&preview_path_str, processor).await {
            Ok(text) if !text.trim().is_empty() => {
                sections.push(format!(
                    "--- DOCX 内嵌 Visio 预览图：{} ---\n{}",
                    preview_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("preview"),
                    text.trim()
                ));
            }
            Ok(_) => errors.push(format!("{}: 未识别到文本", preview_path.display())),
            Err(error) => errors.push(format!("{}: {}", preview_path.display(), error)),
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);

    if sections.is_empty() {
        return Err(format!(
            "DOCX 内嵌 Visio 预览图 OCR 失败：{}",
            errors.join("；")
        ));
    }

    Ok(sections.join("\n\n"))
}

async fn extract_text_with_docx_preview_fallback(
    context: &RecompileContext,
    path: &Path,
) -> Result<String, String> {
    match crate::services::file_extractor::extract_text(path) {
        Ok(text) => Ok(text),
        Err(error)
            if is_docx_path(path)
                && crate::services::file_extractor::is_unreadable_docx_embedded_object_error(
                    &error,
                ) =>
        {
            tracing::warn!("重编译读取 DOCX 失败，尝试内嵌 Visio 预览图 OCR: {}", error);
            extract_docx_preview_text(context, path).await
        }
        Err(error) => Err(error),
    }
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
            &context,
            Path::new(&source.storage_path),
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
    let context = RecompileContext::from_state(&state);
    let text = extract_text_with_docx_preview_fallback(&context, Path::new(&source.storage_path))
        .await
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
