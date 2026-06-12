use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use anyhow::{Context as _, anyhow};
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};
use crate::services::analysis_cache::AnalysisCacheStore;
use crate::services::ingest_cache::IngestCacheStore;
use crate::services::ingestion::{
    ingest_directory as ingest_directory_fn, ingest_text as ingest_text_fn,
    DirectoryIngestionResult, IngestionResult,
};
use crate::services::ingestion_helpers::extract_title_from_filename;
use crate::services::ingestion_pipeline::process_with_kb_compilation;
use crate::services::llm_providers::LLMProviderManager;
use crate::services::wiki_page::WikiPageStore;

/// 从全局状态克隆 ImageProcessor 配置（不持锁，避免 Send 问题）
fn clone_image_processor(
    state: &State<'_, AppState>,
) -> AppResult<crate::services::image_processor::ImageProcessor> {
    let guard = state
        .image_processor
        .read()
        .map_err(|e| anyhow!("获取 image_processor 读锁失败: {}", e))?;
    Ok(guard.clone_configured())
}

/// 通过 ImageProcessor 异步提取图片文本（owned processor，可 Send）
async fn extract_image_text_with_processor(
    file_path: &str,
    processor: crate::services::image_processor::ImageProcessor,
) -> AppResult<String> {
    let result = processor
        .process_image(file_path)
        .await
        .map_err(|e| AppError::Api(format!("图片处理失败: {}", e)))
        .with_context(|| format!("处理图片失败: {}", file_path))?;
    if result.text.trim().is_empty() {
        return Err(AppError::Api(format!(
            "图片中未识别到任何文本: {}",
            file_path
        )));
    }
    Ok(result.text)
}

fn is_docx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("docx"))
        .unwrap_or(false)
}

fn create_docx_preview_temp_dir(file_path: &Path) -> AppResult<PathBuf> {
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("docx");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "kingdee-kb-docx-preview-{}-{}-{}",
        std::process::id(),
        nonce,
        stem
    ));
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::io(&dir, e))
        .with_context(|| format!("创建 DOCX 预览图临时目录失败: {}", dir.display()))?;
    Ok(dir)
}

async fn extract_docx_preview_text(
    state: &State<'_, AppState>,
    file_path: &Path,
) -> AppResult<String> {
    let can_process = {
        let guard = state
            .image_processor
            .read()
            .map_err(|e| anyhow!("获取 image_processor 读锁失败: {}", e))?;
        guard.can_process_images()
    };
    if !can_process {
        return Err(AppError::Config(
            "DOCX 内嵌 Visio 无法直接提取文字，且未配置 OCR 或多模态视觉模型".into(),
        ));
    }

    let temp_dir = create_docx_preview_temp_dir(file_path)?;
    let preview_paths = crate::services::file_extractor::extract_docx_preview_images(
        file_path, &temp_dir,
    )
    .map_err(|e| AppError::Api(format!("提取 DOCX 预览图失败: {}", e)))?;
    let mut sections = Vec::new();
    let mut errors = Vec::new();

    for preview_path in preview_paths {
        let preview_path_str = preview_path.to_string_lossy().to_string();
        let processor = clone_image_processor(state)?;
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
        return Err(AppError::Api(format!(
            "DOCX 内嵌 Visio 预览图 OCR 失败：{}",
            errors.join("；")
        )));
    }

    Ok(sections.join("\n\n"))
}

async fn extract_text_with_docx_preview_fallback(
    state: &State<'_, AppState>,
    path: &Path,
) -> AppResult<String> {
    match crate::services::file_extractor::extract_text(path) {
        Ok(text) => Ok(text),
        Err(error)
            if is_docx_path(path)
                && crate::services::file_extractor::is_unreadable_docx_embedded_object_error(
                    &error,
                ) =>
        {
            tracing::warn!("DOCX 文本提取失败，尝试内嵌 Visio 预览图 OCR: {}", error);
            extract_docx_preview_text(state, path).await
        }
        Err(error) => Err(AppError::Api(format!("文件文本提取失败: {}", error))),
    }
}

fn ingest_file_text(
    state: &State<'_, AppState>,
    app: &AppHandle,
    path: &Path,
    text: &str,
    project_id: i64,
) -> AppResult<IngestionResult> {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled");
    let title = extract_title_from_filename(filename);
    let file_path = path.to_string_lossy().to_string();
    let mut result = ingest_text_fn(
        text,
        &title,
        project_id,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&state.raw_sources),
        Some(filename),
        Some(&file_path),
        Some(app),
        Some(&state.data_dir),
    )
    .map_err(|e| AppError::Other(anyhow!("ingest_text_fn 失败: {}", e)))?;
    result.title = title;
    Ok(result)
}

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

/// 执行 KB 编译并返回 (engine, error) 元组
///
/// 抽取公共逻辑消除四处重复的 process_with_kb_compilation 调用模式。
async fn run_kb_compilation(
    text: &str,
    source_identity: &str,
    sha256: &str,
    project_id: i64,
    title: &str,
    document_id: i64,
    analysis_cache: Arc<Mutex<AnalysisCacheStore>>,
    llm_providers: Arc<RwLock<LLMProviderManager>>,
    wiki_pages: Arc<Mutex<WikiPageStore>>,
    ingest_cache_store: Arc<Mutex<IngestCacheStore>>,
) -> (Option<String>, Option<String>) {
    match process_with_kb_compilation(
        text,
        source_identity,
        sha256,
        project_id,
        title,
        true,
        analysis_cache,
        llm_providers,
        wiki_pages,
        ingest_cache_store,
        Some(document_id),
        false,
    )
    .await
    {
        Ok(compilation) => (Some(compilation.engine), None),
        Err(e) => {
            tracing::warn!("KB 编译失败（{}）: {}", title, e);
            (None, Some(format!("{}", e)))
        }
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
    project_id: i64,
    enable_kb_compilation: Option<bool>,
) -> Result<IngestionResult, String> {
    state.ensure_embedding_ready();
    state.ensure_bm25_ready();

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
        let (engine, error) = run_kb_compilation(
            &text,
            &source_identity,
            &result.sha256,
            project_id,
            &title,
            result.document_id,
            state.analysis_cache.clone(),
            state.llm_providers.clone(),
            state.wiki_pages.clone(),
            state.ingest_cache_store.clone(),
        )
        .await;
        result.kb_analysis_engine = engine;
        result.kb_compilation_error = error;
    }

    Ok(result)
}

/// 摄入单个文件
///
/// 首次调用时会自动加载 embedding 模型（懒加载）。
/// 图片文件（PNG/JPG/GIF/BMP/WEBP）会通过 OCR/多模态视觉自动提取文本后摄入。
#[tauri::command]
pub async fn ingest_file(
    state: State<'_, AppState>,
    app: AppHandle,
    file_path: String,
    project_id: i64,
    enable_kb_compilation: Option<bool>,
) -> Result<IngestionResult, String> {
    state.ensure_embedding_ready();
    state.ensure_bm25_ready();

    let path = PathBuf::from(&file_path);

    // ─── 图片文件分支：通过 ImageProcessor 异步提取文本 ───
    if crate::services::file_extractor::is_image_format(&path) {
        // 同步块：检查能力 + 克隆 processor（不持锁跨 await）
        let can_process = {
            let guard = state.image_processor.read().map_err(|e| e.to_string())?;
            guard.can_process_images()
        };
        if !can_process {
            return Err("未配置 OCR 或多模态视觉模型，无法提取图片文本".to_string());
        }
        let processor = clone_image_processor(&state).map_err(|e| e.to_string())?;

        // 异步：提取图片文本
        let image_text = extract_image_text_with_processor(&file_path, processor)
            .await
            .map_err(|e| e.to_string())?;

        // 同步：走纯文本摄入流程
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled");
        let title = filename
            .rsplit_once('.')
            .map(|(n, _)| n)
            .unwrap_or(filename)
            .replace(['-', '_'], " ");

        let mut result = ingest_text_fn(
            &image_text,
            &title,
            project_id,
            &state.embedding,
            &state.vector_index,
            &state.metadata,
            &state.bm25,
            Some(&state.raw_sources),
            Some(filename),
            Some(&file_path),
            Some(&app),
            Some(&state.data_dir),
        )?;
        result.title = title.clone();

        // KB 编译
        if resolve_kb_compilation(&state, enable_kb_compilation) {
            let source_identity = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let (engine, error) = run_kb_compilation(
                &image_text,
                &source_identity,
                &result.sha256,
                project_id,
                &title,
                result.document_id,
                state.analysis_cache.clone(),
                state.llm_providers.clone(),
                state.wiki_pages.clone(),
                state.ingest_cache_store.clone(),
            )
            .await;
            result.kb_analysis_engine = engine;
            result.kb_compilation_error = error;
        }

        return Ok(result);
    }

    // ─── 非图片文件：先提取文本，DOCX 内嵌 Visio 失败时自动走预览图 OCR ───
    let text = extract_text_with_docx_preview_fallback(&state, path.as_path())
        .await
        .map_err(|e: AppError| e.to_string())?;
    let mut result = ingest_file_text(&state, &app, path.as_path(), &text, project_id)
        .map_err(|e: AppError| e.to_string())?;

    // 知识编译
    if resolve_kb_compilation(&state, enable_kb_compilation) {
        let title = result.title.clone();
        let source_identity = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let (engine, error) = run_kb_compilation(
            &text,
            &source_identity,
            &result.sha256,
            project_id,
            &title,
            result.document_id,
            state.analysis_cache.clone(),
            state.llm_providers.clone(),
            state.wiki_pages.clone(),
            state.ingest_cache_store.clone(),
        )
        .await;
        result.kb_analysis_engine = engine;
        result.kb_compilation_error = error;
    }

    Ok(result)
}

#[tauri::command]
pub async fn extract_file_text(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<ExtractedFileText, String> {
    let path = PathBuf::from(&file_path);
    let text = extract_text_with_docx_preview_fallback(&state, &path)
        .await
        .map_err(|e| e.to_string())?;
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
/// 图片文件（PNG/JPG/GIF/BMP/WEBP）会通过 OCR/多模态视觉自动提取文本后摄入。
#[tauri::command]
pub async fn ingest_directory(
    state: State<'_, AppState>,
    app: AppHandle,
    dir_path: String,
    project_id: i64,
    enable_kb_compilation: Option<bool>,
) -> Result<DirectoryIngestionResult, String> {
    state.ensure_embedding_ready();
    state.ensure_bm25_ready();

    let dir = PathBuf::from(&dir_path);

    // 同步摄入非图片文件（图片在同步扫描中已被跳过）
    let mut result = ingest_directory_fn(
        dir.as_path(),
        project_id,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        Some(&state.raw_sources),
        Some(&app),
        Some(&state.data_dir),
    )?;

    // ─── 对同步提取失败的 DOCX 内嵌 Visio 文档做预览图 OCR 回退 ───
    let mut retained_errors = Vec::new();
    for error in std::mem::take(&mut result.errors) {
        let path = PathBuf::from(&error.path);
        let can_fallback = is_docx_path(&path)
            && crate::services::file_extractor::is_unreadable_docx_embedded_object_error(
                &error.error,
            )
            && path.exists();
        if !can_fallback {
            retained_errors.push(error);
            continue;
        }

        match extract_text_with_docx_preview_fallback(&state, path.as_path())
            .await
            .map_err(|e: AppError| e.to_string())
        {
            Ok(text) => match ingest_file_text(&state, &app, path.as_path(), &text, project_id)
                .map_err(|e: AppError| e.to_string())
            {
                Ok(imported) => result.imported.push(imported),
                Err(import_error) => retained_errors.push(crate::services::ingestion::FileError {
                    path: error.path,
                    error: import_error,
                }),
            },
            Err(ocr_error) => retained_errors.push(crate::services::ingestion::FileError {
                path: error.path,
                error: ocr_error,
            }),
        }
    }
    result.errors = retained_errors;

    // ─── 异步处理图片文件 ───
    let image_paths = crate::services::ingestion::collect_image_paths(&dir);
    let can_process_images = {
        let guard = state.image_processor.read().map_err(|e| e.to_string())?;
        guard.can_process_images()
    };

    if !image_paths.is_empty() && can_process_images {
        for img_path in &image_paths {
            let img_path_str = img_path.to_string_lossy().to_string();

            // 同步：克隆 processor
            let processor = match clone_image_processor(&state) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("克隆 ImageProcessor 失败: {}", e);
                    result.errors.push(crate::services::ingestion::FileError {
                        path: img_path_str.clone(),
                        error: e.to_string(),
                    });
                    continue;
                }
            };

            // 异步：提取图片文本
            let image_text = match extract_image_text_with_processor(&img_path_str, processor).await
            {
                Ok(text) => text,
                Err(e) => {
                    tracing::warn!("图片 OCR 失败: {:?}: {}", img_path, e);
                    result.errors.push(crate::services::ingestion::FileError {
                        path: img_path_str.clone(),
                        error: e.to_string(),
                    });
                    continue;
                }
            };

            // 同步：走纯文本摄入
            let filename = img_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("untitled");
            let title = filename
                .rsplit_once('.')
                .map(|(n, _)| n)
                .unwrap_or(filename)
                .replace(['-', '_'], " ");

            match ingest_text_fn(
                &image_text,
                &title,
                project_id,
                &state.embedding,
                &state.vector_index,
                &state.metadata,
                &state.bm25,
                Some(&state.raw_sources),
                Some(filename),
                Some(&img_path_str),
                Some(&app),
                Some(&state.data_dir),
            ) {
                Ok(mut ing_result) => {
                    ing_result.title = title;
                    result.imported.push(ing_result);
                }
                Err(e) => {
                    tracing::warn!("图片摄入失败: {:?}: {}", img_path, e);
                    result.errors.push(crate::services::ingestion::FileError {
                        path: img_path_str.clone(),
                        error: e,
                    });
                }
            }
        }
    } else if !image_paths.is_empty() && !can_process_images {
        // 有图片但未配置 OCR/视觉模型，记录为错误
        for img_path in &image_paths {
            result.errors.push(crate::services::ingestion::FileError {
                path: img_path.to_string_lossy().to_string(),
                error: "未配置 OCR 或多模态视觉模型，无法提取图片文本".to_string(),
            });
        }
    }

    // ─── 知识编译（对每个成功导入的文件，包括图片） ───
    if resolve_kb_compilation(&state, enable_kb_compilation) {
        for imported in &mut result.imported {
            if let Some(ref sp) = imported.source_path {
                let path_buf = PathBuf::from(sp);

                // 获取文本：优先使用缓存的 extracted_text（避免重复 OCR/读取）
                let text = if let Some(ref cached_text) = imported.extracted_text {
                    cached_text.clone()
                } else {
                    let is_image = crate::services::file_extractor::is_image_format(&path_buf);
                    if is_image {
                        let processor = match clone_image_processor(&state) {
                            Ok(p) => p,
                            Err(e) => {
                                imported.kb_compilation_error = Some(e.to_string());
                                continue;
                            }
                        };
                        match extract_image_text_with_processor(sp, processor).await {
                            Ok(t) => t,
                            Err(e) => {
                                imported.kb_compilation_error = Some(e.to_string());
                                continue;
                            }
                        }
                    } else {
                        match crate::services::file_extractor::extract_text(&path_buf) {
                            Ok(t) => t,
                            Err(e) => {
                                imported.kb_compilation_error =
                                    Some(format!("读取文件失败: {}", e));
                                continue;
                            }
                        }
                    }
                };

                let sha256 = imported.sha256.clone();
                let title = imported.title.clone();
                let source_identity = path_buf
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let (engine, error) = run_kb_compilation(
                    &text,
                    &source_identity,
                    &sha256,
                    project_id,
                    &title,
                    imported.document_id,
                    state.analysis_cache.clone(),
                    state.llm_providers.clone(),
                    state.wiki_pages.clone(),
                    state.ingest_cache_store.clone(),
                )
                .await;
                imported.kb_analysis_engine = engine;
                imported.kb_compilation_error = error;
            }
        }
    }

    Ok(result)
}
