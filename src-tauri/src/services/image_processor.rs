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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionFallback {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

pub struct ImageProcessor {
    ocr_config: Option<OcrConfig>,
    client: reqwest::Client,
    llm_api_key: String,
    llm_base_url: String,
    llm_model: String,
    vision_fallback: Option<VisionFallback>,
    llm_multimodal: Arc<AtomicBool>,
    probed: Arc<AtomicBool>,
}

impl ImageProcessor {
    pub fn new(llm_api_key: String, llm_base_url: String, llm_model: String) -> Self {
        Self {
            ocr_config: None,
            client: reqwest::Client::new(),
            llm_api_key,
            llm_base_url,
            llm_model,
            vision_fallback: None,
            llm_multimodal: Arc::new(AtomicBool::new(false)),
            probed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_ocr_config(&mut self, config: OcrConfig) {
        self.ocr_config = Some(config);
    }

    pub fn set_vision_fallback(&mut self, config: VisionFallback) {
        self.vision_fallback = Some(config);
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
                if resp.status().is_success() {
                    true
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    let lower = body.to_lowercase();
                    !(lower.contains("image") || lower.contains("vision") || lower.contains("multimodal"))
                }
            }
            Err(_) => false,
        };

        self.llm_multimodal.store(is_multimodal, Ordering::Relaxed);
        self.probed.store(true, Ordering::Relaxed);
        is_multimodal
    }

    pub fn is_llm_multimodal(&self) -> bool {
        self.llm_multimodal.load(Ordering::Relaxed)
    }

    pub fn has_ocr(&self) -> bool {
        self.ocr_config.is_some()
    }

    pub fn can_process_images(&self) -> bool {
        self.is_llm_multimodal() || self.vision_fallback.is_some() || self.ocr_config.is_some()
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
                    self.ocr(&img_base64, config).await?
                } else {
                    self.vision_ocr(&img_base64).await?
                }
            }
            ImageType::Flowchart | ImageType::Architecture => {
                let prompt = if img_type == ImageType::Flowchart {
                    "请详细描述这个流程图：起始/结束节点、步骤名称、判断条件、整体目的"
                } else {
                    "请详细描述这个架构图：组件名称、关系依赖、数据流向、设计思路"
                };
                self.vision(&img_base64, prompt).await?
            }
            ImageType::Table => {
                self.vision(&img_base64, "请将表格提取为结构化文本，保留行列关系").await?
            }
            ImageType::Mixed => {
                self.vision(&img_base64, "请描述这张图片的内容").await?
            }
        };

        Ok(ImageContent { image_type: img_type, text, processing_time_ms: start.elapsed().as_millis() as u64 })
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

    async fn ocr(&self, img_base64: &str, config: &OcrConfig) -> Result<String, ImageError> {
        match config.provider {
            OcrProvider::Baidu => self.ocr_baidu(img_base64, config).await,
            OcrProvider::Tencent => self.ocr_tencent(img_base64, config).await,
            OcrProvider::Llm => self.vision_ocr(img_base64).await,
        }
    }

    async fn vision_ocr(&self, img_base64: &str) -> Result<String, ImageError> {
        self.vision(img_base64, "请识别并提取图片中的所有文字，保持原始格式").await
    }

    async fn ocr_baidu(&self, img_base64: &str, config: &OcrConfig) -> Result<String, ImageError> {
        let token = self.get_baidu_token(config).await?;
        let resp = self.client
            .post("https://aip.baidubce.com/rest/2.0/ocr/v1/general_basic")
            .query(&[("access_token", &token)])
            .form(&[("image", img_base64)])
            .send().await.map_err(|e| ImageError::ApiError(e.to_string()))?;
        let result: serde_json::Value = resp.json().await.map_err(|e| ImageError::ApiError(e.to_string()))?;
        let words: Vec<String> = result["words_result"].as_array().unwrap_or(&vec![])
            .iter().map(|w| w["words"].as_str().unwrap_or("").to_string()).collect();
        Ok(words.join("\n"))
    }

    async fn get_baidu_token(&self, config: &OcrConfig) -> Result<String, ImageError> {
        let secret = config.secret_key.as_ref().ok_or(ImageError::OcrNotConfigured)?;
        let resp = self.client
            .post("https://aip.baidubce.com/oauth/2.0/token")
            .query(&[("grant_type", "client_credentials"), ("client_id", &config.api_key), ("client_secret", secret)])
            .send().await.map_err(|e| ImageError::ApiError(e.to_string()))?;
        let result: serde_json::Value = resp.json().await.map_err(|e| ImageError::ApiError(e.to_string()))?;
        result["access_token"].as_str().map(|s| s.to_string())
            .ok_or_else(|| ImageError::ApiError("获取百度 token 失败".to_string()))
    }

    async fn ocr_tencent(&self, img_base64: &str, config: &OcrConfig) -> Result<String, ImageError> {
        let resp = self.client
            .post("https://ocr.tencentcloudapi.com")
            .header("Content-Type", "application/json")
            .header("X-TC-Action", "GeneralBasicOCR")
            .header("X-TC-Version", "2018-11-19")
            .header("X-TC-Region", "ap-guangzhou")
            .header("Authorization", format!("TC3-HMAC-SHA256 Credential={}", config.api_key))
            .json(&serde_json::json!({"ImageBase64": img_base64}))
            .send().await.map_err(|e| ImageError::ApiError(e.to_string()))?;
        let result: serde_json::Value = resp.json().await.map_err(|e| ImageError::ApiError(e.to_string()))?;
        let words: Vec<String> = result["Response"]["TextDetections"].as_array().unwrap_or(&vec![])
            .iter().map(|w| w["DetectedText"].as_str().unwrap_or("").to_string()).collect();
        Ok(words.join("\n"))
    }

    /// LLM 图像理解（自动选择：主 LLM 或备用配置）
    async fn vision(&self, img_base64: &str, prompt: &str) -> Result<String, ImageError> {
        let (api_key, base_url, model) = if self.is_llm_multimodal() {
            (&self.llm_api_key, &self.llm_base_url, &self.llm_model)
        } else if let Some(ref fb) = self.vision_fallback {
            (&fb.api_key, &fb.base_url, &fb.model)
        } else {
            return Err(ImageError::LlmNotMultimodal);
        };

        if api_key.is_empty() {
            return Err(ImageError::LlmNotConfigured);
        }

        let resp = self.client
            .post(format!("{}/chat/completions", base_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": prompt},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", img_base64)}}
                ]}],
                "max_tokens": 1000
            }))
            .send().await.map_err(|e| ImageError::ApiError(e.to_string()))?;

        let result: serde_json::Value = resp.json().await.map_err(|e| ImageError::ApiError(e.to_string()))?;
        result["choices"][0]["message"]["content"].as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ImageError::ApiError("LLM 返回为空".to_string()))
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
        assert_eq!(serde_json::to_string(&ImageType::Flowchart).unwrap(), "\"Flowchart\"");
    }

    #[test]
    fn test_image_content_serialization() {
        let content = ImageContent { image_type: ImageType::TextScreenshot, text: "Hello".to_string(), processing_time_ms: 100 };
        assert!(serde_json::to_string(&content).unwrap().contains("Hello"));
    }

    #[test]
    fn test_compute_image_hash() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.png");
        std::fs::write(&path, b"fake image data").unwrap();
        assert!(!ImageProcessor::compute_image_hash(path.to_str().unwrap()).unwrap().is_empty());
    }
}
