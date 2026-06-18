//! OCR 多供应商路由：百度 / 腾讯 / Mistral / LLM OCR + 降级链

use super::{ImageContent, ImageError, ImageProcessor, OcrConfig, OcrProvider};

/// Mistral OCR 默认 base_url
pub const MISTRAL_OCR_DEFAULT_BASE_URL: &str = "https://api.mistral.ai/v1";

/// 是否配置了 Mistral OCR
fn has_mistral(ocr_config: &Option<OcrConfig>) -> bool {
    ocr_config
        .as_ref()
        .is_some_and(|c| c.provider == OcrProvider::Mistral)
}

/// graph/table 类处理：Mistral OCR 优先（表格/图表强），否则 LLM 多模态 vision → 百度/腾讯 OCR 兜底
pub async fn ocr_or_fallback(
    processor: &ImageProcessor,
    img_base64: &str,
    img_bytes: &[u8],
    local_path: Option<&str>,
    prompt: &str,
) -> Result<String, ImageError> {
    if has_mistral(&processor.ocr_config) {
        ocr_mistral(processor, img_base64, img_bytes).await
    } else {
        vision_or_ocr_fallback(processor, img_base64, img_bytes, local_path, prompt).await
    }
}

/// graph/table 类的降级链：LLM 多模态 vision → 失败则百度/腾讯 OCR 兜底
async fn vision_or_ocr_fallback(
    processor: &ImageProcessor,
    img_base64: &str,
    img_bytes: &[u8],
    local_path: Option<&str>,
    prompt: &str,
) -> Result<String, ImageError> {
    match super::vision::vision(processor, img_base64, img_bytes, local_path, prompt).await {
        Ok(text) if !text.trim().is_empty() => Ok(text),
        Ok(_) | Err(_) => {
            // LLM 多模态不可用或返回空 → 百度/腾讯 OCR 兜底
            if let Some(ref config) = processor.ocr_config {
                if matches!(config.provider, OcrProvider::Baidu | OcrProvider::Tencent) {
                    return ocr(processor, img_base64, img_bytes, local_path, config).await;
                }
            }
            // 无其他 OCR → 再试 vision_ocr 作为最后手段
            super::vision::vision_ocr(processor, img_base64, img_bytes, local_path).await
        }
    }
}

/// OCR 专用处理（绕过类型分类的 vision 路由，直接走 OCR）
///
/// 用于所有 vision 候选模型失败后的最终回退。
/// - Baidu/Tencent OCR provider: 纯 OCR，不涉及 LLM
/// - LLM OCR provider (OcrProvider::Llm): 仍会调用 vision_ocr()，即最后一次 LLM 尝试
/// - 无 OCR 配置: fallback 到 vision_ocr() 作为最后手段
pub async fn ocr_only(processor: &ImageProcessor, path: &str) -> Result<ImageContent, ImageError> {
    let start = std::time::Instant::now();
    let img_bytes = std::fs::read(path).map_err(|e| ImageError::IoError(e.to_string()))?;
    let img_base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &img_bytes);
    let img_type = super::image_classify::classify_image(&img_bytes)?;

    let text = if let Some(ref config) = processor.ocr_config {
        match config.provider {
            OcrProvider::Baidu | OcrProvider::Tencent => {
                ocr(processor, &img_base64, &img_bytes, Some(path), config).await?
            }
            OcrProvider::Mistral => {
                ocr_mistral(processor, &img_base64, &img_bytes).await?
            }
            OcrProvider::Llm => {
                super::vision::vision_ocr(processor, &img_base64, &img_bytes, Some(path)).await?
            }
        }
    } else {
        // 无 OCR 配置 → 用 LLM vision 做 OCR（最后手段，可能仍失败）
        super::vision::vision_ocr(processor, &img_base64, &img_bytes, Some(path)).await?
    };

    let markdown_inline = super::image_classify::build_inline_markdown(&img_type, &text);
    Ok(ImageContent {
        image_type: img_type,
        text,
        processing_time_ms: start.elapsed().as_millis() as u64,
        markdown_inline,
    })
}

/// OCR 分发路由
pub(super) async fn ocr(
    processor: &ImageProcessor,
    img_base64: &str,
    img_bytes: &[u8],
    local_path: Option<&str>,
    config: &OcrConfig,
) -> Result<String, ImageError> {
    match config.provider {
        OcrProvider::Baidu => ocr_baidu(processor, img_base64, config).await,
        OcrProvider::Tencent => ocr_tencent(processor, img_base64, config).await,
        OcrProvider::Mistral => ocr_mistral(processor, img_base64, img_bytes).await,
        OcrProvider::Llm => super::vision::vision_ocr(processor, img_base64, img_bytes, local_path).await,
    }
}

/// 调用 Mistral OCR（mistral-ocr-latest）处理单张图片
///
/// API: `POST {base_url}/ocr`，document 传 image_url（`data:image/<mime>;base64,...`）。
/// 返回 `pages[0].markdown`，含表格转 Markdown、图注。表格/图表/版式理解强。
/// MIME 必须由 img_bytes 推断，不能硬编码（官方 cookbook 用 jpeg）。
async fn ocr_mistral(
    processor: &ImageProcessor,
    img_base64: &str,
    img_bytes: &[u8],
) -> Result<String, ImageError> {
    let config = processor
        .ocr_config
        .as_ref()
        .ok_or_else(|| ImageError::OcrError("Mistral OCR 未配置".to_string()))?;
    if !matches!(config.provider, OcrProvider::Mistral) {
        return Err(ImageError::OcrError("当前 OCR 配置非 Mistral".to_string()));
    }
    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or(MISTRAL_OCR_DEFAULT_BASE_URL)
        .trim_end_matches('/');
    let url = format!("{}/ocr", base_url);

    // MIME 从图片实际格式推断（image crate guess_format），默认 png 兜底
    let mime = image::guess_format(img_bytes)
        .map(|f| f.to_mime_type())
        .unwrap_or("image/png");
    let data_url = format!("data:{};base64,{}", mime, img_base64);

    let body = serde_json::json!({
        "model": "mistral-ocr-latest",
        "document": {
            "type": "image_url",
            "image_url": data_url,
        }
    });

    let resp = processor.client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ImageError::OcrError(format!("Mistral OCR 请求失败: {}", e)))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| ImageError::OcrError(format!("Mistral OCR 读取响应失败: {}", e)))?;
    if !status.is_success() {
        return Err(ImageError::OcrError(format!(
            "Mistral OCR HTTP {}: {}",
            status,
            text.chars().take(500).collect::<String>()
        )));
    }

    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| ImageError::OcrError(format!("Mistral OCR 响应解析失败: {}", e)))?;

    // 响应结构：{ pages: [ { markdown: "..." } ] }
    let markdown = json
        .get("pages")
        .and_then(|p| p.get(0))
        .and_then(|page| page.get("markdown"))
        .and_then(|m| m.as_str())
        .ok_or_else(|| {
            ImageError::OcrError(format!("Mistral OCR 响应无 pages[0].markdown: {}", text.chars().take(200).collect::<String>()))
        })?;

    Ok(markdown.trim().to_string())
}

async fn ocr_baidu(
    processor: &ImageProcessor,
    img_base64: &str,
    config: &OcrConfig,
) -> Result<String, ImageError> {
    let token = get_baidu_token(processor, config).await?;
    let resp = processor.client
        .post("https://aip.baidubce.com/rest/2.0/ocr/v1/general_basic")
        .query(&[("access_token", &token)])
        .form(&[("image", img_base64)])
        .send()
        .await
        .map_err(|e| ImageError::ApiError(e.to_string()))?
        .error_for_status()
        .map_err(|e| {
            let status = e.status().map(|s| s.to_string()).unwrap_or_else(|| "N/A".to_string());
            ImageError::ApiError(format!("百度 OCR HTTP 错误（status={}）: {}", status, e))
        })?;
    let result: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ImageError::ApiError(e.to_string()))?;
    // 百度错误：error_code + error_msg 顶层（非 words_result）
    if let Some(code) = result["error_code"].as_i64() {
        let msg = result["error_msg"].as_str().unwrap_or("");
        return Err(ImageError::ApiError(format!("百度 OCR 错误 {code}: {msg}")));
    }
    let words: Vec<String> = result["words_result"]
        .as_array()
        .map_or(&[][..], |v| v)
        .iter()
        .map(|w| w["words"].as_str().unwrap_or("").to_string())
        .collect();
    Ok(words.join("\n"))
}

async fn get_baidu_token(
    processor: &ImageProcessor,
    config: &OcrConfig,
) -> Result<String, ImageError> {
    let secret = config
        .secret_key
        .as_ref()
        .ok_or(ImageError::OcrNotConfigured)?;
    let resp = processor.client
        .post("https://aip.baidubce.com/oauth/2.0/token")
        .query(&[
            ("grant_type", "client_credentials"),
            ("client_id", &config.api_key),
            ("client_secret", secret),
        ])
        .send()
        .await
        .map_err(|e| ImageError::ApiError(e.to_string()))?
        .error_for_status()
        .map_err(|e| {
            let status = e.status().map(|s| s.to_string()).unwrap_or_else(|| "N/A".to_string());
            ImageError::ApiError(format!("百度 OAuth HTTP 错误（status={}）: {}", status, e))
        })?;
    let result: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ImageError::ApiError(e.to_string()))?;
    if let Some(err) = result["error"].as_str() {
        let desc = result["error_description"].as_str().unwrap_or("");
        return Err(ImageError::ApiError(format!("百度 OAuth 错误 {err}: {desc}")));
    }
    result["access_token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| ImageError::ApiError("获取百度 token 失败".to_string()))
}

async fn ocr_tencent(
    processor: &ImageProcessor,
    img_base64: &str,
    config: &OcrConfig,
) -> Result<String, ImageError> {
    // 腾讯云 OCR：SecretId=config.api_key, SecretKey=config.secret_key
    let secret_key = config
        .secret_key
        .as_ref()
        .ok_or(ImageError::OcrNotConfigured)?;
    let secret_id = &config.api_key;

    let payload = serde_json::json!({"ImageBase64": img_base64}).to_string();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // TC3-HMAC-SHA256 签名（service=ocr, host=ocr.tencentcloudapi.com, action=generalbasicocr）
    let canonical_request = format!(
        "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:ocr.tencentcloudapi.com\nx-tc-action:generalbasicocr\n\ncontent-type;host;x-tc-action\n{}",
        super::sha256_hex(&payload)
    );
    let credential_scope = format!("{}/ocr/tc3_request", date);
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{}\n{}\n{}",
        timestamp,
        credential_scope,
        super::sha256_hex(&canonical_request)
    );
    let secret_date = super::hmac_sha256(format!("TC3{}", secret_key).as_bytes(), date.as_bytes());
    let secret_service = super::hmac_sha256(&secret_date, b"ocr");
    let secret_signing = super::hmac_sha256(&secret_service, b"tc3_request");
    let signature = hex::encode(super::hmac_sha256(&secret_signing, string_to_sign.as_bytes()));
    let authorization = format!(
        "TC3-HMAC-SHA256 Credential={}/{}, SignedHeaders=content-type;host;x-tc-action, Signature={}",
        secret_id, date, signature
    );

    let resp = processor.client
        .post("https://ocr.tencentcloudapi.com")
        .header("Content-Type", "application/json; charset=utf-8")
        .header("X-TC-Action", "GeneralBasicOCR")
        .header("X-TC-Version", "2018-11-19")
        .header("X-TC-Region", "ap-guangzhou")
        .header("X-TC-Timestamp", timestamp.to_string())
        .header("Authorization", authorization)
        .body(payload)
        .send()
        .await
        .map_err(|e| ImageError::ApiError(e.to_string()))?;
    let result: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ImageError::ApiError(e.to_string()))?;

    // 腾讯云错误：Response.Error
    if let Some(err) = result["Response"]["Error"].as_object() {
        let code = err.get("Code").and_then(|c| c.as_str()).unwrap_or("Unknown");
        let msg = err.get("Message").and_then(|m| m.as_str()).unwrap_or("");
        return Err(ImageError::ApiError(format!("腾讯 OCR 失败: {} {}", code, msg)));
    }

    let words: Vec<String> = result["Response"]["TextDetections"]
        .as_array()
        .map_or(&[][..], |v| v)
        .iter()
        .map(|w| w["DetectedText"].as_str().unwrap_or("").to_string())
        .collect();
    Ok(words.join("\n"))
}
