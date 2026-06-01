//! 图像处理模块 — OCR + 多模态 LLM 理解
//!
//! 智能复用 LLM 配置：
//!   - 通过 API 探测判断当前 LLM 是否支持多模态
//!   - 支持多模态 → 直接复用 LLM 配置
//!   - 不支持 → 使用备用 Vision 配置或专用 OCR

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ─── 类型定义 ───

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ImageType {
    TextScreenshot,
    Flowchart,
    Architecture,
    Table,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    pub image_type: ImageType,
    pub text: String,
    pub processing_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    pub provider: OcrProvider,
    pub api_key: String,
    pub secret_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OcrProvider {
    Baidu,
    Tencent,
    Llm,
}

pub struct ImageProcessor {
    ocr_config: Option<OcrConfig>,
    client: reqwest::Client,
    llm_api_key: String,
    llm_base_url: String,
    llm_model: String,
    llm_multimodal: Arc<AtomicBool>,
    probed: Arc<AtomicBool>,
    protocol: Option<crate::services::llm_providers::LLMProtocol>,
}

impl ImageProcessor {
    pub fn new(llm_api_key: String, llm_base_url: String, llm_model: String) -> Self {
        Self {
            ocr_config: None,
            client: reqwest::Client::new(),
            llm_api_key,
            llm_base_url,
            llm_model,
            llm_multimodal: Arc::new(AtomicBool::new(false)),
            probed: Arc::new(AtomicBool::new(false)),
            protocol: None,
        }
    }

    pub fn set_ocr_config(&mut self, config: OcrConfig) {
        self.ocr_config = Some(config);
    }

    pub fn set_protocol(&mut self, protocol: crate::services::llm_providers::LLMProtocol) {
        self.protocol = Some(protocol);
    }

    pub fn is_llm_multimodal(&self) -> bool {
        self.llm_multimodal.load(Ordering::Relaxed)
    }

    pub fn set_llm_multimodal(&mut self, value: bool) {
        self.llm_multimodal.store(value, Ordering::Relaxed);
        self.probed.store(true, Ordering::Relaxed);
    }

    /// 获取 LLM API Key
    pub fn get_llm_api_key(&self) -> &str {
        &self.llm_api_key
    }

    /// 获取 LLM Base URL
    pub fn get_llm_base_url(&self) -> &str {
        &self.llm_base_url
    }

    /// 获取 LLM 模型名
    pub fn get_llm_model(&self) -> &str {
        &self.llm_model
    }

    /// 克隆 OCR 配置
    pub fn get_ocr_config_cloned(&self) -> Option<OcrConfig> {
        self.ocr_config.clone()
    }

    /// 探测当前 LLM 是否支持多模态
    pub async fn probe_multimodal(&self) -> bool {
        if self.probed.load(Ordering::Relaxed) {
            return self.llm_multimodal.load(Ordering::Relaxed);
        }

        if self.llm_api_key.is_empty() {
            self.probed.store(true, Ordering::Relaxed);
            return false;
        }

        // 非 OpenAI 协议不在此探测（由多模型回退机制在实际 vision 调用中处理）
        if let Some(ref proto) = self.protocol {
            if *proto != crate::services::llm_providers::LLMProtocol::OpenAI
                && *proto != crate::services::llm_providers::LLMProtocol::Local
            {
                self.probed.store(true, Ordering::Relaxed);
                return false;
            }
        }

        // 用 1x1 透明图片测试
        let test_img = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

        let result = self.client
            .post(format!("{}/chat/completions", self.llm_base_url))
            .header("Authorization", format!("Bearer {}", self.llm_api_key))
            .json(&serde_json::json!({
                "model": self.llm_model,
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": "test"},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", test_img)}}
                ]}],
                "max_tokens": 1
            }))
            .send()
            .await;

        let is_multimodal = match result {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    // 非 2xx 状态码 → 不支持多模态
                    tracing::info!(
                        "Multimodal probe failed with status {} for model {}",
                        status, self.llm_model
                    );
                    false
                } else {
                    // 2xx 状态码还需检查响应体：某些 API 代理/网关会返回 200 但 body 含错误
                    match resp.text().await {
                        Ok(body) => {
                            // 尝试解析为 JSON，检查是否有 choices（正常响应）
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                                if json["choices"].is_array() {
                                    true
                                } else if json["error"].is_object() {
                                    // OpenAI 兼容 API 在 body 中返回错误
                                    tracing::info!(
                                        "Multimodal probe got 200 but error in body for model {}: {:?}",
                                        self.llm_model, json["error"]
                                    );
                                    false
                                } else {
                                    // 有响应但格式不标准，保守认为支持
                                    tracing::info!(
                                        "Multimodal probe got unexpected response format for model {}, assuming multimodal",
                                        self.llm_model
                                    );
                                    true
                                }
                            } else {
                                // 非 JSON 响应，保守认为支持（某些本地模型返回非标准格式）
                                tracing::info!(
                                    "Multimodal probe got non-JSON response for model {}, assuming multimodal",
                                    self.llm_model
                                );
                                true
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Multimodal probe failed to read response body for model {}: {:?}",
                                self.llm_model, e
                            );
                            false
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Multimodal probe HTTP error for model {}: {:?}",
                    self.llm_model, e
                );
                false
            }
        };

        self.llm_multimodal.store(is_multimodal, Ordering::Relaxed);
        self.probed.store(true, Ordering::Relaxed);
        is_multimodal
    }

    pub fn has_ocr(&self) -> bool {
        self.ocr_config.is_some()
    }

    pub fn can_process_images(&self) -> bool {
        self.is_llm_multimodal() || self.ocr_config.is_some()
    }

    pub fn get_ocr_provider(&self) -> Option<String> {
        self.ocr_config.as_ref().map(|c| match c.provider {
            OcrProvider::Baidu => "baidu".to_string(),
            OcrProvider::Tencent => "tencent".to_string(),
            OcrProvider::Llm => "llm".to_string(),
        })
    }

    pub fn update_llm_config(&mut self, api_key: String, base_url: String, model: String) {
        self.llm_api_key = api_key;
        self.llm_base_url = base_url;
        self.llm_model = model;
        self.probed.store(false, Ordering::Relaxed);
    }

    pub async fn process_image(&self, path: &str) -> Result<ImageContent, ImageError> {
        let start = std::time::Instant::now();
        let img_bytes = std::fs::read(path).map_err(|e| ImageError::IoError(e.to_string()))?;
        let img_base64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);
        let img_type = self.classify_image(&img_bytes)?;

        let text = match img_type {
            ImageType::TextScreenshot => {
                if let Some(ref config) = self.ocr_config {
                    self.ocr(&img_base64, Some(path), config).await?
                } else {
                    self.vision_ocr(&img_base64, Some(path)).await?
                }
            }
            ImageType::Flowchart | ImageType::Architecture => {
                let prompt = if img_type == ImageType::Flowchart {
                    "请详细描述这个流程图：起始/结束节点、步骤名称、判断条件、整体目的"
                } else {
                    "请详细描述这个架构图：组件名称、关系依赖、数据流向、设计思路"
                };
                self.vision(&img_base64, Some(path), prompt).await?
            }
            ImageType::Table => {
                self.vision(
                    &img_base64,
                    Some(path),
                    "请将表格提取为结构化文本，保留行列关系",
                )
                .await?
            }
            ImageType::Mixed => {
                self.vision(&img_base64, Some(path), "请描述这张图片的内容")
                    .await?
            }
        };

        Ok(ImageContent {
            image_type: img_type,
            text,
            processing_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// OCR 专用处理（绕过类型分类的 vision 路由，直接走 OCR）
    ///
    /// 用于所有 vision 候选模型失败后的最终回退。
    /// - Baidu/Tencent OCR provider: 纯 OCR，不涉及 LLM
    /// - LLM OCR provider (OcrProvider::Llm): 仍会调用 vision_ocr()，即最后一次 LLM 尝试
    /// - 无 OCR 配置: fallback 到 vision_ocr() 作为最后手段
    pub async fn ocr_only(&self, path: &str) -> Result<ImageContent, ImageError> {
        let start = std::time::Instant::now();
        let img_bytes = std::fs::read(path).map_err(|e| ImageError::IoError(e.to_string()))?;
        let img_base64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);
        let img_type = self.classify_image(&img_bytes)?;

        let text = if let Some(ref config) = self.ocr_config {
            match config.provider {
                OcrProvider::Baidu | OcrProvider::Tencent => {
                    // 专用 OCR 服务（百度/腾讯），纯 OCR 不涉及 LLM
                    self.ocr(&img_base64, Some(path), config).await?
                }
                OcrProvider::Llm => {
                    // LLM OCR: 最后一次 LLM 尝试（用 OCR 提示词调用 vision）
                    self.vision_ocr(&img_base64, Some(path)).await?
                }
            }
        } else {
            // 无 OCR 配置 → 用 LLM vision 做 OCR（最后手段，可能仍失败）
            self.vision_ocr(&img_base64, Some(path)).await?
        };

        Ok(ImageContent {
            image_type: img_type,
            text,
            processing_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn classify_image(&self, img_bytes: &[u8]) -> Result<ImageType, ImageError> {
        let img = image::load_from_memory(img_bytes)
            .map_err(|e| ImageError::FormatError(e.to_string()))?;
        let (w, h) = (img.width(), img.height());
        let ratio = w as f64 / h as f64;

        if w < 100 || h < 100 {
            Ok(ImageType::Mixed)
        } else if ratio > 1.5 || ratio < 0.67 {
            Ok(ImageType::Flowchart)
        } else {
            Ok(ImageType::Mixed)
        }
    }

    async fn ocr(
        &self,
        img_base64: &str,
        local_path: Option<&str>,
        config: &OcrConfig,
    ) -> Result<String, ImageError> {
        match config.provider {
            OcrProvider::Baidu => self.ocr_baidu(img_base64, config).await,
            OcrProvider::Tencent => self.ocr_tencent(img_base64, config).await,
            OcrProvider::Llm => self.vision_ocr(img_base64, local_path).await,
        }
    }

    async fn vision_ocr(
        &self,
        img_base64: &str,
        local_path: Option<&str>,
    ) -> Result<String, ImageError> {
        self.vision(
            img_base64,
            local_path,
            "请识别并提取图片中的所有文字，保持原始格式",
        )
        .await
    }

    async fn ocr_baidu(&self, img_base64: &str, config: &OcrConfig) -> Result<String, ImageError> {
        let token = self.get_baidu_token(config).await?;
        let resp = self
            .client
            .post("https://aip.baidubce.com/rest/2.0/ocr/v1/general_basic")
            .query(&[("access_token", &token)])
            .form(&[("image", img_base64)])
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;
        let words: Vec<String> = result["words_result"]
            .as_array()
            .map_or(&[][..], |v| v)
            .iter()
            .map(|w| w["words"].as_str().unwrap_or("").to_string())
            .collect();
        Ok(words.join("\n"))
    }

    async fn get_baidu_token(&self, config: &OcrConfig) -> Result<String, ImageError> {
        let secret = config
            .secret_key
            .as_ref()
            .ok_or(ImageError::OcrNotConfigured)?;
        let resp = self
            .client
            .post("https://aip.baidubce.com/oauth/2.0/token")
            .query(&[
                ("grant_type", "client_credentials"),
                ("client_id", &config.api_key),
                ("client_secret", secret),
            ])
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;
        result["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ImageError::ApiError("获取百度 token 失败".to_string()))
    }

    async fn ocr_tencent(
        &self,
        img_base64: &str,
        config: &OcrConfig,
    ) -> Result<String, ImageError> {
        let resp = self
            .client
            .post("https://ocr.tencentcloudapi.com")
            .header("Content-Type", "application/json")
            .header("X-TC-Action", "GeneralBasicOCR")
            .header("X-TC-Version", "2018-11-19")
            .header("X-TC-Region", "ap-guangzhou")
            .header(
                "Authorization",
                format!("TC3-HMAC-SHA256 Credential={}", config.api_key),
            )
            .json(&serde_json::json!({"ImageBase64": img_base64}))
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;
        let result: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;
        let words: Vec<String> = result["Response"]["TextDetections"]
            .as_array()
            .map_or(&[][..], |v| v)
            .iter()
            .map(|w| w["DetectedText"].as_str().unwrap_or("").to_string())
            .collect();
        Ok(words.join("\n"))
    }

/// LLM 图像理解（直接尝试调用，不预先探测）
    async fn vision(
        &self,
        img_base64: &str,
        local_path: Option<&str>,
        prompt: &str,
    ) -> Result<String, ImageError> {
        if self.llm_api_key.is_empty() {
            return Err(ImageError::LlmNotMultimodal);
        }

        // 如果之前已确认不支持多模态，直接返回错误（避免重复无效请求）
        if self.probed.load(Ordering::Relaxed) && !self.llm_multimodal.load(Ordering::Relaxed) {
            return Err(ImageError::LlmNotMultimodal);
        }

        let protocol = self.protocol.as_ref().unwrap_or(&crate::services::llm_providers::LLMProtocol::OpenAI);

        match protocol {
            crate::services::llm_providers::LLMProtocol::Anthropic => {
                self.vision_anthropic(img_base64, local_path, prompt).await
            }
            crate::services::llm_providers::LLMProtocol::OpenAI
            | crate::services::llm_providers::LLMProtocol::Local => {
                self.vision_openai_compatible(img_base64, local_path, prompt).await
            }
        }
    }

    /// Anthropic Messages API 格式的视觉调用
    async fn vision_anthropic(
        &self,
        img_base64: &str,
        local_path: Option<&str>,
        prompt: &str,
    ) -> Result<String, ImageError> {
        let url = format!("{}/v1/messages", self.llm_base_url.trim_end_matches('/'));

        let media_type = local_path
            .and_then(|p| std::path::Path::new(p).extension())
            .and_then(|e| e.to_str())
            .map(|e| match e.to_lowercase().as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "gif" => "image/gif",
                "webp" => "image/webp",
                _ => "image/png",
            })
            .unwrap_or("image/png");

        let resp = self.client
            .post(&url)
            .header("x-api-key", &self.llm_api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "model": self.llm_model,
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
                    let json: serde_json::Value = r.json().await
                        .map_err(|e| ImageError::ApiError(format!("Anthropic 响应解析失败: {}", e)))?;
                    let text = json["content"][0]["text"].as_str()
                        .ok_or_else(|| ImageError::ApiError("Anthropic API: 响应中无文本内容".to_string()))?;
                    if text.is_empty() {
                        return Err(ImageError::ApiError("LLM 返回内容为空".to_string()));
                    }
                    self.llm_multimodal.store(true, Ordering::Relaxed);
                    self.probed.store(true, Ordering::Relaxed);
                    Ok(text.to_string())
                } else {
                    let err_text = r.text().await.unwrap_or_default();
                    let err_lower = err_text.to_lowercase();
                    if err_lower.contains("image") || err_lower.contains("vision")
                        || err_lower.contains("multimodal") || err_lower.contains("not supported")
                    {
                        self.llm_multimodal.store(false, Ordering::Relaxed);
                        self.probed.store(true, Ordering::Relaxed);
                        return Err(ImageError::LlmNotMultimodal);
                    }
                    Err(ImageError::ApiError(format!(
                        "Anthropic API HTTP {} ({} > {}): {}",
                        status, self.llm_base_url, self.llm_model,
                        &err_text.chars().take(300).collect::<String>()
                    )))
                }
            }
            Err(e) => Err(ImageError::ApiError(format!(
                "Anthropic 请求失败 ({} > {}): {:?}", self.llm_base_url, self.llm_model, e
            ))),
        }
    }

    /// OpenAI 兼容格式的视觉调用（原有实现）
    async fn vision_openai_compatible(
        &self,
        img_base64: &str,
        local_path: Option<&str>,
        prompt: &str,
    ) -> Result<String, ImageError> {
        let api_key = &self.llm_api_key;
        let base_url = &self.llm_base_url;
        let model = &self.llm_model;

        let url = format!("{}/chat/completions", base_url);
        let mut last_api_error: Option<String> = None;

        // ─── 1. 尝试 Base64 ───
        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": prompt},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", img_base64)}}
                ]}],
                "max_tokens": 2048
            }))
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    let body = r.text().await.unwrap_or_default();
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        if json["choices"].is_array() {
                            self.llm_multimodal.store(true, Ordering::Relaxed);
                            self.probed.store(true, Ordering::Relaxed);
                            let content = json["choices"][0]["message"]["content"]
                                .as_str().unwrap_or("").to_string();
                            if content.is_empty() {
                                return Err(ImageError::ApiError("LLM 返回内容为空".to_string()));
                            }
                            return Ok(content);
                        } else if json["error"].is_object() {
                            let err_msg = format!("API 返回错误: {}", json["error"]);
                            tracing::warn!("Vision got 200 but error in body for model {}: {}", model, err_msg);
                            if err_msg.contains("image") || err_msg.contains("vision") || err_msg.contains("multimodal") {
                                self.llm_multimodal.store(false, Ordering::Relaxed);
                                self.probed.store(true, Ordering::Relaxed);
                                return Err(ImageError::LlmNotMultimodal);
                            }
                            last_api_error = Some(err_msg);
                        }
                    }
                    last_api_error = Some(format!("响应格式异常 (HTTP 200), body 前200字: {}",
                        &body.chars().take(200).collect::<String>()));
                } else {
                    let err_text = r.text().await.unwrap_or_default();
                    let err_lower = err_text.to_lowercase();
                    if err_lower.contains("image") || err_lower.contains("vision")
                        || err_lower.contains("multimodal") || err_lower.contains("does not support")
                        || err_lower.contains("not supported")
                        || status.as_u16() == 400 || status.as_u16() == 422
                    {
                        self.llm_multimodal.store(false, Ordering::Relaxed);
                        self.probed.store(true, Ordering::Relaxed);
                        return Err(ImageError::LlmNotMultimodal);
                    }
                    last_api_error = Some(format!("HTTP {} ({} > {}): {}",
                        status, base_url, model,
                        &err_text.chars().take(300).collect::<String>()));
                }
            }
            Err(e) => {
                last_api_error = Some(format!("请求失败 ({} > {}): {:?}", base_url, model, e));
            }
        }

        // ─── 2. 尝试本地 file:/// 路径回退 ───
        if let Some(path) = local_path {
            let absolute_path = std::path::Path::new(path)
                .canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(path));
            let file_url = format!("file:///{}", absolute_path.to_string_lossy().replace('\\', "/"));

            let resp_local = self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&serde_json::json!({
                    "model": model,
                    "messages": [{"role": "user", "content": [
                        {"type": "text", "text": prompt},
                        {"type": "image_url", "image_url": {"url": file_url}}
                    ]}],
                    "max_tokens": 2048
                }))
                .send()
                .await;

            match resp_local {
                Ok(r) => {
                    let status = r.status();
                    if status.is_success() {
                        let body = r.text().await.unwrap_or_default();
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                            if json["choices"].is_array() {
                                self.llm_multimodal.store(true, Ordering::Relaxed);
                                self.probed.store(true, Ordering::Relaxed);
                                let content = json["choices"][0]["message"]["content"]
                                    .as_str().unwrap_or("").to_string();
                                if content.is_empty() {
                                    return Err(ImageError::ApiError("LLM 返回内容为空".to_string()));
                                }
                                return Ok(content);
                            } else if json["error"].is_object() {
                                let err_msg = format!("API 返回错误: {}", json["error"]);
                                if err_msg.contains("image") || err_msg.contains("vision") || err_msg.contains("multimodal") {
                                    self.llm_multimodal.store(false, Ordering::Relaxed);
                                    self.probed.store(true, Ordering::Relaxed);
                                    return Err(ImageError::LlmNotMultimodal);
                                }
                                last_api_error = Some(err_msg);
                            }
                        }
                        last_api_error = Some(format!("file:// 回退响应格式异常, body 前200字: {}",
                            &body.chars().take(200).collect::<String>()));
                    } else {
                        let err_text = r.text().await.unwrap_or_default();
                        let err_lower = err_text.to_lowercase();
                        if err_lower.contains("image") || err_lower.contains("vision")
                            || err_lower.contains("multimodal") || err_lower.contains("does not support")
                            || err_lower.contains("not supported")
                        {
                            self.llm_multimodal.store(false, Ordering::Relaxed);
                            self.probed.store(true, Ordering::Relaxed);
                            return Err(ImageError::LlmNotMultimodal);
                        }
                        // 非 200 但非多模态不支持 → 保留错误信息，不再降级
                        last_api_error = Some(format!("file:// 回退 HTTP {} ({} > {}): {}",
                            status, base_url, model,
                            &err_text.chars().take(300).collect::<String>()));
                    }
                }
                Err(e) => {
                    last_api_error = Some(format!("file:// 回退请求失败 ({} > {}): {:?}", base_url, model, e));
                }
            }
        }

        // 返回实际 API 错误，而非笼统信息
        let detail = last_api_error.unwrap_or_else(|| "未知错误".to_string());
        Err(ImageError::ApiError(format!(
            "多模态图像识别失败 ({} > {}): {}", base_url, model, detail
        )))
    }

    pub fn compute_image_hash(path: &str) -> Result<String, ImageError> {
        let bytes = std::fs::read(path).map_err(|e| ImageError::IoError(e.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("IO 错误: {0}")]
    IoError(String),
    #[error("API 错误: {0}")]
    ApiError(String),
    #[error("OCR 未配置")]
    OcrNotConfigured,
    #[error("LLM 未配置")]
    LlmNotConfigured,
    #[error("LLM 不支持多模态，需配置备用 Vision 提供商")]
    LlmNotMultimodal,
    #[error("图像格式错误: {0}")]
    FormatError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_type_serialization() {
        assert_eq!(
            serde_json::to_string(&ImageType::Flowchart).unwrap(),
            "\"Flowchart\""
        );
    }

    #[test]
    fn test_image_content_serialization() {
        let content = ImageContent {
            image_type: ImageType::TextScreenshot,
            text: "Hello".to_string(),
            processing_time_ms: 100,
        };
        assert!(serde_json::to_string(&content).unwrap().contains("Hello"));
    }

    #[test]
    fn test_compute_image_hash() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.png");
        std::fs::write(&path, b"fake image data").unwrap();
        assert!(!ImageProcessor::compute_image_hash(path.to_str().unwrap())
            .unwrap()
            .is_empty());
    }
}
