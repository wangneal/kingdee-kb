//! 图像分类：基于宽高比的快速兜底分类 + 内联 Markdown 构造

use super::{ImageError, ImageType};

/// 判断图片类型是否在排除列表中
pub fn is_excluded(excluded_image_types: &[String], img_type: &ImageType) -> bool {
    excluded_image_types
        .iter()
        .any(|t| t == img_type.to_category())
}

/// 构造位置内联 Markdown：`![图(type)](描述)`
pub fn build_inline_markdown(img_type: &ImageType, text: &str) -> String {
    let category = img_type.to_category();
    let desc = text.trim();
    if desc.is_empty() {
        String::new()
    } else {
        format!("![图({})]({})", category, desc.replace('\n', " "))
    }
}

/// 图像类型分类入口（当前仅使用启发式宽高比分类）
pub fn classify_image(img_bytes: &[u8]) -> Result<ImageType, ImageError> {
    classify_image_heuristic(img_bytes)
}

/// 宽高比快速兜底分类（保留原逻辑，作为语义分类不可用时的兜底）
fn classify_image_heuristic(img_bytes: &[u8]) -> Result<ImageType, ImageError> {
    let img = image::load_from_memory(img_bytes)
        .map_err(|e| ImageError::FormatError(e.to_string()))?;
    let (w, h) = (img.width(), img.height());
    let ratio = w as f64 / h as f64;

    if w < 100 || h < 100 {
        Ok(ImageType::Image)
    } else if ratio > 1.5 || ratio < 0.67 {
        Ok(ImageType::Flowchart)
    } else {
        Ok(ImageType::Image)
    }
}
