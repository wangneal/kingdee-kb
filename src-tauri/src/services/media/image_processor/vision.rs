//! LLM 视觉调用：Anthropic / Ollama / OpenAI 兼容协议 + VisionStrategy 路由

use super::{ImageError, ImageProcessor};
use std::sync::atomic::Ordering;

/// OpenAI 兼容视觉请求的图片传递策略
#[derive(Debug, Clone, Copy)]
pub(super) enum VisionStrategy {
    /// Base64 内联图片（data:image/...;base64,...）
    Base64,
    /// 本地 file:// URL（适用于部分内网部署的 OpenAI 兼容代理）
    FileUrl,
}

/// 单次视觉请求的错误类型
#[derive(Debug)]
enum VisionAttemptError {
    /// 明确"模型不支持多模态"（含 image/vision/multimodal/not supported/does not support 关键字）
    NotMultimodal,
    /// 其他 API 错误（HTTP 非 2xx、网络失败、空响应等）
    Other(String),
}

/// LLM 图像理解（直接尝试调用，不预先探测）
pub async fn vision(
    processor: &ImageProcessor,
    img_base64: &str,
    img_bytes: &[u8],
    local_path: Option<&str>,
    prompt: &str,
) -> Result<String, ImageError> {
    if processor.requires_api_key() {
        return Err(ImageError::LlmNotMultimodal);
    }

    // 如果之前已确认不支持多模态，直接返回错误（避免重复无效请求）
    if processor.probed.load(Ordering::Relaxed) && !processor.llm_multimodal.load(Ordering::Relaxed) {
        return Err(ImageError::LlmNotMultimodal);
    }

    let protocol = processor
        .protocol
        .as_ref()
        .unwrap_or(&crate::services::llm_providers::LLMProtocol::OpenAI);

    match protocol {
        crate::services::llm_providers::LLMProtocol::Anthropic => {
            vision_anthropic(processor, img_base64, img_bytes, local_path, prompt).await
        }
        crate::services::llm_providers::LLMProtocol::Local => {
            vision_ollama(processor, img_base64, prompt).await
        }
        crate::services::llm_providers::LLMProtocol::OpenAI => {
            vision_openai_compatible(processor, img_base64, img_bytes, local_path, prompt)
                .await
        }
    }
}

/// OCR 专用处理（绕过类型分类，直接调 vision 做 OCR）
pub async fn vision_ocr(
    processor: &ImageProcessor,
    img_base64: &str,
    img_bytes: &[u8],
    local_path: Option<&str>,
) -> Result<String, ImageError> {
    vision(
        processor,
        img_base64,
        img_bytes,
        local_path,
        "请识别并提取图片中的所有文字，保持原始格式",
    )
    .await
}

/// Ollama 原生格式的视觉调用
async fn vision_ollama(
    processor: &ImageProcessor,
    img_base64: &str,
    prompt: &str,
) -> Result<String, ImageError> {
    let url = format!("{}/api/chat", processor.llm_base_url.trim_end_matches('/'));
    let response = processor.client
        .post(&url)
        .json(&serde_json::json!({
            "model": processor.llm_model,
            "messages": [{
                "role": "user",
                "content": prompt,
                "images": [img_base64]
            }],
            "stream": false
        }))
        .send()
        .await
        .map_err(|e| ImageError::ApiError(format!("Ollama 请求失败: {}", e)))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        if ImageProcessor::err_body_indicates_no_multimodal_support(&body) {
            return processor.mark_llm_non_multimodal_and_error();
        }
        return Err(ImageError::ApiError(format!(
            "Ollama API 返回错误 ({}): {}",
            status,
            body.chars().take(300).collect::<String>()
        )));
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| ImageError::ApiError(format!("Ollama 响应解析失败: {}", e)))?;
    let content = json["message"]["content"]
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| ImageError::ApiError("Ollama 响应中无文本内容".to_string()))?;

    processor.llm_multimodal.store(true, Ordering::Relaxed);
    processor.probed.store(true, Ordering::Relaxed);
    Ok(content.to_string())
}

/// Anthropic Messages API 格式的视觉调用
async fn vision_anthropic(
    processor: &ImageProcessor,
    img_base64: &str,
    img_bytes: &[u8],
    _local_path: Option<&str>,
    prompt: &str,
) -> Result<String, ImageError> {
    let url = crate::services::llm_providers::anthropic_messages_url(&processor.llm_base_url);

    // Anthropic 仅支持 jpeg/png/gif/webp；其他格式兜底 png
    let media_type = image::guess_format(img_bytes)
        .map(|f| match f {
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Gif => "image/gif",
            image::ImageFormat::WebP => "image/webp",
            _ => "image/png",
        })
        .unwrap_or("image/png");

    let resp = processor.client
        .post(&url)
        .header("x-api-key", &processor.llm_api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": processor.llm_model,
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": media_type, "data": img_base64}},
                {"type": "text", "text": prompt}
            ]}]
        }))
        .send()
        .await;

    match resp {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                let json: serde_json::Value = r.json().await.map_err(|e| {
                    ImageError::ApiError(format!("Anthropic 响应解析失败: {}", e))
                })?;
                let text = json["content"][0]["text"].as_str().ok_or_else(|| {
                    ImageError::ApiError("Anthropic API: 响应中无文本内容".to_string())
                })?;
                if text.is_empty() {
                    return Err(ImageError::ApiError("LLM 返回内容为空".to_string()));
                }
                processor.llm_multimodal.store(true, Ordering::Relaxed);
                processor.probed.store(true, Ordering::Relaxed);
                Ok(text.to_string())
            } else {
                let err_text = r.text().await.unwrap_or_default();
                if ImageProcessor::err_body_indicates_no_multimodal_support(&err_text) {
                    return processor.mark_llm_non_multimodal_and_error();
                }
                Err(ImageError::ApiError(format!(
                    "Anthropic API HTTP {} ({} > {}): {}",
                    status,
                    processor.llm_base_url,
                    processor.llm_model,
                    &err_text.chars().take(300).collect::<String>()
                )))
            }
        }
        Err(e) => Err(ImageError::ApiError(format!(
            "Anthropic 请求失败 ({} > {}): {:?}",
            processor.llm_base_url, processor.llm_model, e
        ))),
    }
}

/// OpenAI 兼容协议的视觉调用（GPT/Qwen/DeepSeek 等）
async fn vision_openai_compatible(
    processor: &ImageProcessor,
    img_base64: &str,
    img_bytes: &[u8],
    local_path: Option<&str>,
    prompt: &str,
) -> Result<String, ImageError> {
    if processor.requires_api_key() {
        return Err(ImageError::LlmNotMultimodal);
    }

    // 已确认不支持多模态 → 直接返回
    if processor.probed.load(Ordering::Relaxed) && !processor.llm_multimodal.load(Ordering::Relaxed) {
        return Err(ImageError::LlmNotMultimodal);
    }

    // 尝试 1: Base64 内联
    match try_vision_request(processor, VisionStrategy::Base64, img_base64, img_bytes, local_path, prompt).await {
        Ok(text) => Ok(text),
        Err(VisionAttemptError::NotMultimodal) => Err(ImageError::LlmNotMultimodal),
        Err(VisionAttemptError::Other(_)) if local_path.is_some() => {
            // Base64 失败但有本地路径 → 回退到 file://
            match try_vision_request(processor, VisionStrategy::FileUrl, img_base64, img_bytes, local_path, prompt).await {
                Ok(text) => Ok(text),
                Err(VisionAttemptError::NotMultimodal) => Err(ImageError::LlmNotMultimodal),
                Err(VisionAttemptError::Other(detail)) => {
                    Err(ImageError::ApiError(format!(
                        "多模态图像识别失败 ({} > {}): Base64 尝试 + file:// 回退均失败，最后错误: {}",
                        processor.llm_base_url, processor.llm_model, detail
                    )))
                }
            }
        }
        Err(VisionAttemptError::Other(detail)) => {
            Err(ImageError::ApiError(format!(
                "多模态图像识别失败 ({} > {}): {}",
                processor.llm_base_url, processor.llm_model, detail
            )))
        }
    }
}

/// OpenAI 兼容协议视觉请求的单次尝试
///
/// `strategy` 决定图片内容如何传给 API：
/// - `Base64`: 内联为 data:image/...;base64,...
/// - `FileUrl`: 用 file:/// 本地路径（适用于部分内网代理支持）
///
/// 返回：
/// - `Ok(String)`: LLM 返回的视觉描述文本
/// - `Err(VisionAttemptError::NotMultimodal)`: 模型明确不支持多模态，调用方应停止重试
/// - `Err(VisionAttemptError::Other(msg))`: 其他错误（含详细诊断信息）
async fn try_vision_request(
    processor: &ImageProcessor,
    strategy: VisionStrategy,
    img_base64: &str,
    img_bytes: &[u8],
    local_path: Option<&str>,
    prompt: &str,
) -> Result<String, VisionAttemptError> {
    let api_key = &processor.llm_api_key;
    let base_url = &processor.llm_base_url;
    let model = &processor.llm_model;

    // ─── 1. 构造请求 URL 和 body ───
    let url = format!("{}/chat/completions", base_url);
    let image_url = match strategy {
        VisionStrategy::Base64 => {
            let mime = image::guess_format(img_bytes)
                .map(|f| f.to_mime_type())
                .unwrap_or("image/png");
            format!("data:{};base64,{}", mime, img_base64)
        }
        VisionStrategy::FileUrl => {
            let path = local_path.ok_or_else(|| {
                VisionAttemptError::Other("file:// 策略需要 local_path 参数".to_string())
            })?;
            let absolute_path = std::path::Path::new(path)
                .canonicalize()
                .map_err(|e| {
                    tracing::warn!("file:// 回退：canonicalize 失败 {}: {}", path, e);
                    VisionAttemptError::Other(format!("file:// canonicalize 失败: {}", e))
                })?;
            format!(
                "file:///{}",
                absolute_path.to_string_lossy().replace('\\', "/")
            )
        }
    };

    let mut req = processor.client.post(&url).json(&serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": prompt},
            {"type": "image_url", "image_url": {"url": image_url}}
        ]}],
        "max_tokens": 2048
    }));
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    // ─── 2. 发送并解析 ───
    let resp = req.send().await.map_err(|e| {
        VisionAttemptError::Other(format!("请求失败 ({} > {}): {:?}", base_url, model, e))
    })?;
    let status = resp.status();
    if !status.is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        if ImageProcessor::err_body_indicates_no_multimodal_support(&err_text)
            || matches!(status.as_u16(), 400 | 422)
        {
            processor.llm_multimodal.store(false, Ordering::Relaxed);
            processor.probed.store(true, Ordering::Relaxed);
            return Err(VisionAttemptError::NotMultimodal);
        }
        return Err(VisionAttemptError::Other(format!(
            "HTTP {} ({} > {}): {}",
            status,
            base_url,
            model,
            &err_text.chars().take(300).collect::<String>()
        )));
    }

    let body = resp.text().await.unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        VisionAttemptError::Other(format!(
            "响应格式异常 (HTTP 200), body 前200字: {}, 解析错误: {}",
            &body.chars().take(200).collect::<String>(),
            e
        ))
    })?;

    if json["choices"].is_array() {
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if content.is_empty() {
            return Err(VisionAttemptError::Other("LLM 返回内容为空".to_string()));
        }
        processor.llm_multimodal.store(true, Ordering::Relaxed);
        processor.probed.store(true, Ordering::Relaxed);
        Ok(content)
    } else if json["error"].is_object() {
        let err_msg = format!("API 返回错误: {}", json["error"]);
        tracing::warn!(
            "Vision got 200 but error in body for model {}: {}",
            model,
            err_msg
        );
        if ImageProcessor::err_body_indicates_no_multimodal_support(&err_msg) {
            processor.llm_multimodal.store(false, Ordering::Relaxed);
            processor.probed.store(true, Ordering::Relaxed);
            Err(VisionAttemptError::NotMultimodal)
        } else {
            Err(VisionAttemptError::Other(err_msg))
        }
    } else {
        Err(VisionAttemptError::Other(format!(
            "响应格式异常 (HTTP 200), body 前200字: {}",
            &body.chars().take(200).collect::<String>()
        )))
    }
}
