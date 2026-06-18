//! DOCX 内嵌 Visio 预览图 OCR 回退链共享辅助函数
//!
//! 把 `commands/ingestion.rs` 和 `commands/kb_compilation.rs` 中重复的 6 个函数
//! 合并到本模块，避免 bug 修复改一边忘另一边。
//!
//! 使用方式：两个 commands 文件 `use crate::services::docx_image_helpers::*`

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context as _, anyhow};

use crate::error::{AppError, AppResult};
use crate::services::image_processor::{ImageContent, ImageProcessor};

/// 判断路径是否为 .docx 后缀（不区分大小写）
pub fn is_docx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("docx"))
        .unwrap_or(false)
}

/// 创建 DOCX 预览图临时目录（按用途加前缀避免冲突）
///
/// `purpose` 形如 `"ingest"` 或 `"recompile"`，会拼进目录名以避免并发场景冲突
pub fn create_docx_preview_temp_dir(file_path: &Path, purpose: &str) -> AppResult<PathBuf> {
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("docx");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "kingdee-kb-{}-docx-preview-{}-{}-{}",
        purpose,
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

/// 从 `Arc<RwLock<ImageProcessor>>` 克隆一份 ImageProcessor（不持锁，避免 Send 问题）
pub fn clone_image_processor(
    image_processor: &Arc<RwLock<ImageProcessor>>,
) -> AppResult<ImageProcessor> {
    let guard = image_processor
        .read()
        .map_err(|e| anyhow!("获取 image_processor 读锁失败: {}", e))?;
    Ok(guard.clone_configured())
}

/// 通过 ImageProcessor 异步提取单张图片文本
pub async fn extract_image_text_with_processor(
    file_path: &str,
    processor: ImageProcessor,
) -> AppResult<ImageContent> {
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
    Ok(result)
}

/// 从 `Arc<RwLock<ImageProcessor>>` 异步提取 DOCX 预览图文本（OCR 失败时整体报错）
pub async fn extract_docx_preview_text(
    image_processor: &Arc<RwLock<ImageProcessor>>,
    file_path: &Path,
    purpose: &str,
) -> AppResult<String> {
    let can_process = {
        let guard = image_processor
            .read()
            .map_err(|e| anyhow!("获取 image_processor 读锁失败: {}", e))?;
        guard.can_process_images()
    };
    if !can_process {
        return Err(AppError::Config(
            "DOCX 内嵌 Visio 无法直接提取文字，且未配置 OCR 或多模态视觉模型".into(),
        ));
    }

    let temp_dir = create_docx_preview_temp_dir(file_path, purpose)?;
    let preview_paths = crate::services::file_extractor::extract_docx_preview_images(
        file_path, &temp_dir,
    )
    .map_err(|e| AppError::Api(format!("提取 DOCX 预览图失败: {}", e)))?;
    let mut sections = Vec::new();
    let mut errors = Vec::new();

    for preview_path in preview_paths {
        let preview_path_str = preview_path.to_string_lossy().to_string();
        let processor = clone_image_processor(image_processor)?;
        match extract_image_text_with_processor(&preview_path_str, processor).await {
            Ok(content) if !content.text.trim().is_empty() => {
                sections.push(content.inline_markdown());
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

/// 提取文本（DOCX 内嵌 Visio 失败时自动走预览图 OCR 回退）
pub async fn extract_text_with_docx_preview_fallback(
    image_processor: &Arc<RwLock<ImageProcessor>>,
    path: &Path,
    purpose: &str,
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
            extract_docx_preview_text(image_processor, path, purpose).await
        }
        Err(error) => Err(AppError::Api(format!("文件文本提取失败: {}", error))),
    }
}
