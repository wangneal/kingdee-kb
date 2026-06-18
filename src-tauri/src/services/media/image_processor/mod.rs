//! 图像处理模块 — OCR + 多模态 LLM 理解
//!
//! 智能复用 LLM 配置：
//!   - 通过 API 探测判断当前 LLM 是否支持多模态
//!   - 支持多模态 → 直接复用 LLM 配置
//!   - 不支持 → 使用备用 Vision 配置或专用 OCR

mod image_classify;
mod multimodal_probe;
mod ocr;
mod vision;

use base64::Engine;
use hmac::{Hmac, Mac};
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
    /// 普通图像/照片/装饰图（四分类 image）
    Image,
}

impl ImageType {
    /// 归一化为四分类标签：graph / text / table / image
    ///
    /// - TextScreenshot → text
    /// - Flowchart / Architecture → graph
    /// - Table → table
    /// - Mixed / Image → image
    pub fn to_category(&self) -> &'static str {
        match self {
            ImageType::TextScreenshot => "text",
            ImageType::Flowchart | ImageType::Architecture => "graph",
            ImageType::Table => "table",
            ImageType::Mixed | ImageType::Image => "image",
        }
    }

    /// 从四分类标签反解 ImageType
    pub fn from_category(category: &str) -> Self {
        match category {
            "text" => ImageType::TextScreenshot,
            "graph" => ImageType::Flowchart,
            "table" => ImageType::Table,
            "image" => ImageType::Image,
            _ => ImageType::Mixed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    pub image_type: ImageType,
    pub text: String,
    pub processing_time_ms: u64,
    /// 位置内联 Markdown 描述（`![图(type)](描述)`），供调用方按文档原位置内联
    #[serde(default)]
    pub markdown_inline: String,
}

impl ImageContent {
    /// 取位置内联 Markdown；markdown_inline 为空时按 text 兜底构造
    pub fn inline_markdown(&self) -> String {
        if !self.markdown_inline.is_empty() {
            self.markdown_inline.clone()
        } else {
            format!("![图({})]({})", self.image_type.to_category(), self.text.trim())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    pub provider: OcrProvider,
    pub api_key: String,
    pub secret_key: Option<String>,
    /// OCR 服务 base_url（Mistral 用，如 https://api.mistral.ai/v1；百度/腾讯忽略）
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OcrProvider {
    Baidu,
    Tencent,
    /// Mistral OCR（mistral-ocr-latest，表格/图表/版式强，单图 base64）
    Mistral,
    Llm,
}

/// Mistral OCR 默认 base_url（re-export from ocr submodule）
pub use ocr::MISTRAL_OCR_DEFAULT_BASE_URL;

impl OcrProvider {
    /// 从持久化的 OcrProviderType 转为运行时 OcrProvider
    pub fn from_provider_type(
        t: &crate::services::llm_providers::OcrProviderType,
    ) -> Self {
        match t {
            crate::services::llm_providers::OcrProviderType::Baidu => OcrProvider::Baidu,
            crate::services::llm_providers::OcrProviderType::Tencent => OcrProvider::Tencent,
            crate::services::llm_providers::OcrProviderType::Mistral => OcrProvider::Mistral,
        }
    }

    /// 对应的默认 base_url（仅 Mistral 有，百度/腾讯为 None）
    pub fn default_base_url(&self) -> Option<String> {
        match self {
            OcrProvider::Mistral => Some(MISTRAL_OCR_DEFAULT_BASE_URL.to_string()),
            _ => None,
        }
    }
}

pub struct ImageProcessor {
    ocr_config: Option<OcrConfig>,
    /// 图片处理排除的四分类类型（graph/text/table/image），默认排除 image 装饰图
    excluded_image_types: Vec<String>,
    pub(crate) client: reqwest::Client,
    pub(crate) llm_api_key: String,
    pub(crate) llm_base_url: String,
    pub(crate) llm_model: String,
    pub(crate) llm_multimodal: Arc<AtomicBool>,
    pub(crate) probed: Arc<AtomicBool>,
    pub(crate) protocol: Option<crate::services::llm_providers::LLMProtocol>,
}

impl ImageProcessor {
    pub fn new(llm_api_key: String, llm_base_url: String, llm_model: String) -> Self {
        Self {
            ocr_config: None,
            excluded_image_types: vec!["image".to_string()],
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

    /// 清除 OCR 配置（与 Settings 页"清除 OCR 配置"操作同步）
    pub fn clear_ocr_config(&mut self) {
        self.ocr_config = None;
    }

    /// 设置图片处理排除的类型
    pub fn set_excluded_image_types(&mut self, types: Vec<String>) {
        self.excluded_image_types = types;
    }

    /// 获取图片处理排除的类型
    pub fn get_excluded_image_types(&self) -> Vec<String> {
        self.excluded_image_types.clone()
    }

    pub fn set_protocol(&mut self, protocol: crate::services::llm_providers::LLMProtocol) {
        self.protocol = Some(protocol);
    }

    /// 克隆当前处理器的可复用配置，避免异步调用时持有全局锁。
    pub fn clone_configured(&self) -> Self {
        let mut processor = Self::new(
            self.llm_api_key.clone(),
            self.llm_base_url.clone(),
            self.llm_model.clone(),
        );
        processor.set_llm_multimodal(self.is_llm_multimodal());
        if let Some(ocr) = self.ocr_config.clone() {
            processor.set_ocr_config(ocr);
        }
        processor.excluded_image_types = self.excluded_image_types.clone();
        if let Some(protocol) = self.protocol.clone() {
            processor.set_protocol(protocol);
        }
        processor
    }

    /// 当前配置是否需要 API 密钥才能调用 vision/probe
    /// Local 协议（如 Ollama）不需要密钥
    pub fn requires_api_key(&self) -> bool {
        self.protocol != Some(crate::services::llm_providers::LLMProtocol::Local)
            && self.llm_api_key.is_empty()
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

    /// 克隆协议配置
    pub fn get_protocol_cloned(&self) -> Option<crate::services::llm_providers::LLMProtocol> {
        self.protocol.clone()
    }

    /// 探测当前 LLM 是否支持多模态
    pub async fn probe_multimodal(&self) -> bool {
        multimodal_probe::probe_multimodal(self).await
    }

    pub fn has_ocr(&self) -> bool {
        self.ocr_config.is_some()
    }

    pub fn can_process_images(&self) -> bool {
        self.is_llm_multimodal() || self.ocr_config.is_some() || !self.requires_api_key()
    }

    pub fn get_ocr_provider(&self) -> Option<String> {
        self.ocr_config.as_ref().map(|c| match c.provider {
            OcrProvider::Baidu => "baidu".to_string(),
            OcrProvider::Tencent => "tencent".to_string(),
            OcrProvider::Mistral => "mistral".to_string(),
            OcrProvider::Llm => "llm".to_string(),
        })
    }

    /// 当前图片类型是否被排除（命中 excluded_image_types）
    fn is_excluded(&self, img_type: &ImageType) -> bool {
        image_classify::is_excluded(&self.excluded_image_types, img_type)
    }

    /// 判断 LLM API 错误响应是否为"不支持多模态"
    /// （含 image/vision/multimodal/not supported/does not support 关键字）
    pub(crate) fn err_body_indicates_no_multimodal_support(body: &str) -> bool {
        let lower = body.to_lowercase();
        lower.contains("image")
            || lower.contains("vision")
            || lower.contains("multimodal")
            || lower.contains("not supported")
            || lower.contains("does not support")
    }

    /// 标记 LLM 不支持多模态并返回错误
    pub(crate) fn mark_llm_non_multimodal_and_error(&self) -> Result<String, ImageError> {
        self.llm_multimodal.store(false, Ordering::Relaxed);
        self.probed.store(true, Ordering::Relaxed);
        Err(ImageError::LlmNotMultimodal)
    }

    /// 构造位置内联 Markdown：`![图(type)](描述)`（wrapper to image_classify::build_inline_markdown）
    pub fn build_inline_markdown(img_type: &ImageType, text: &str) -> String {
        image_classify::build_inline_markdown(img_type, text)
    }

    pub fn update_llm_config(&mut self, api_key: String, base_url: String, model: String) {
        self.llm_api_key = api_key;
        self.llm_base_url = base_url;
        self.llm_model = model;
        self.probed.store(false, Ordering::Relaxed);
    }

    /// OCR 专用处理（绕过类型分类的 vision 路由，直接走 OCR）
    ///
    /// 用于所有 vision 候选模型失败后的最终回退。
    pub async fn ocr_only(&self, path: &str) -> Result<ImageContent, ImageError> {
        ocr::ocr_only(self, path).await
    }

    pub async fn process_image(&self, path: &str) -> Result<ImageContent, ImageError> {
        let start = std::time::Instant::now();
        let img_bytes = std::fs::read(path).map_err(|e| ImageError::IoError(e.to_string()))?;
        let img_base64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);
        let img_type = image_classify::classify_image(&img_bytes)?;

        if self.is_excluded(&img_type) {
            return Ok(ImageContent {
                image_type: img_type,
                text: String::new(),
                processing_time_ms: start.elapsed().as_millis() as u64,
                markdown_inline: String::new(),
            });
        }

        // OCR 为主、LLM 多模态为辅：text 走 OCR/vision_ocr，graph/table 走 ocr_or_fallback，image 走 vision
        let text = match img_type {
            ImageType::TextScreenshot => {
                if let Some(ref config) = self.ocr_config {
                    ocr::ocr(self, &img_base64, &img_bytes, Some(path), config).await?
                } else {
                    vision::vision_ocr(self, &img_base64, &img_bytes, Some(path)).await?
                }
            }
            ImageType::Flowchart | ImageType::Architecture => {
                ocr::ocr_or_fallback(
                    self,
                    &img_base64,
                    &img_bytes,
                    Some(path),
                    "请详细描述这个流程图/架构图：节点名称、步骤/组件、关系依赖、数据流向、整体目的",
                )
                .await?
            }
            ImageType::Table => {
                ocr::ocr_or_fallback(
                    self,
                    &img_base64,
                    &img_bytes,
                    Some(path),
                    "请将表格提取为结构化文本，保留行列关系",
                )
                .await?
            }
            ImageType::Mixed | ImageType::Image => {
                vision::vision(self, &img_base64, &img_bytes, Some(path), "请描述这张图片的内容")
                    .await?
            }
        };

        let markdown_inline = Self::build_inline_markdown(&img_type, &text);
        Ok(ImageContent {
            image_type: img_type,
            text,
            processing_time_ms: start.elapsed().as_millis() as u64,
            markdown_inline,
        })
    }

    pub fn compute_image_hash(path: &str) -> Result<String, ImageError> {
        let bytes = std::fs::read(path).map_err(|e| ImageError::IoError(e.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

type HmacSha256 = Hmac<Sha256>;

pub(crate) fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key error");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub(crate) fn sha256_hex(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("IO 错误: {0}")]
    IoError(String),
    #[error("API 错误: {0}")]
    ApiError(String),
    #[error("OCR 错误: {0}")]
    OcrError(String),
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
            markdown_inline: "![图(text)](Hello)".to_string(),
        };
        assert!(serde_json::to_string(&content).unwrap().contains("Hello"));
    }

    #[test]
    fn test_image_type_category_mapping() {
        // 四分类归一化：Architecture 并入 graph，Image/Mixed 归 image
        assert_eq!(ImageType::TextScreenshot.to_category(), "text");
        assert_eq!(ImageType::Flowchart.to_category(), "graph");
        assert_eq!(ImageType::Architecture.to_category(), "graph");
        assert_eq!(ImageType::Table.to_category(), "table");
        assert_eq!(ImageType::Image.to_category(), "image");
        assert_eq!(ImageType::Mixed.to_category(), "image");

        // 反解
        assert_eq!(ImageType::from_category("graph"), ImageType::Flowchart);
        assert_eq!(ImageType::from_category("image"), ImageType::Image);
        assert_eq!(ImageType::from_category("unknown"), ImageType::Mixed);
    }

    #[test]
    fn test_build_inline_markdown() {
        assert_eq!(
            ImageProcessor::build_inline_markdown(&ImageType::Table, "第一行\n第二行"),
            "![图(table)](第一行 第二行)"
        );
        // 空文本 → 空内联
        assert_eq!(ImageProcessor::build_inline_markdown(&ImageType::Image, "  "), "");
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
