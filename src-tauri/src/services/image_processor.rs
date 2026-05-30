//! 图像处理模块 — OCR + 多模态 LLM 理解
//!
//! 支持在线 OCR（百度/腾讯）和多模态 LLM（GPT-4V/通义千问）进行图像理解。
//! 对图片进行分类，选择合适的处理方式：
//!   - 纯文字截图 → OCR
//!   - 流程图/架构图 → 多模态 LLM
//!   - 表格 → OCR + 结构化提取

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ─── 类型定义 ───

/// 图像类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ImageType {
    /// 纯文字截图
    TextScreenshot,
    /// 流程图
    Flowchart,
    /// 架构图
    Architecture,
    /// 表格
    Table,
    /// 混合/其他
    Mixed,
}

/// 图像处理结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    /// 图像类型
    pub image_type: ImageType,
    /// OCR 提取的文本
    pub ocr_text: Option<String>,
    /// LLM 生成的描述
    pub description: Option<String>,
    /// 结构化数据（JSON）
    pub structured_data: Option<serde_json::Value>,
    /// 处理耗时（毫秒）
    pub processing_time_ms: u64,
}

/// OCR 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    /// OCR 提供商
    pub provider: OcrProvider,
    /// API Key
    pub api_key: String,
    /// Secret Key（百度需要）
    pub secret_key: Option<String>,
}

/// OCR 提供商
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OcrProvider {
    /// 百度 OCR
    Baidu,
    /// 腾讯 OCR
    Tencent,
    /// 本地 Tesseract（可选）
    Tesseract,
}

/// 多模态 LLM 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionConfig {
    /// LLM 提供商
    pub provider: VisionProvider,
    /// API Key
    pub api_key: String,
    /// 自定义 Base URL（可选）
    pub base_url: Option<String>,
}

/// 多模态 LLM 提供商
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VisionProvider {
    /// OpenAI GPT-4V
    Gpt4v,
    /// 通义千问 VL
    QwenVl,
    /// 智谱 GLM-4V
    Glm4v,
    /// Claude Vision
    Claude,
}

/// 图像处理器
pub struct ImageProcessor {
    /// OCR 配置
    ocr_config: Option<OcrConfig>,
    /// 多模态 LLM 配置
    vision_config: Option<VisionConfig>,
    /// HTTP 客户端
    client: reqwest::Client,
    /// 缓存目录
    cache_dir: std::path::PathBuf,
}

// ─── 实现 ───

impl ImageProcessor {
    /// 创建图像处理器
    pub fn new(cache_dir: std::path::PathBuf) -> Self {
        Self {
            ocr_config: None,
            vision_config: None,
            client: reqwest::Client::new(),
            cache_dir,
        }
    }

    /// 设置 OCR 配置
    pub fn set_ocr_config(&mut self, config: OcrConfig) {
        self.ocr_config = Some(config);
    }

    /// 设置多模态 LLM 配置
    pub fn set_vision_config(&mut self, config: VisionConfig) {
        self.vision_config = Some(config);
    }

    /// 检查是否配置了 OCR
    pub fn has_ocr(&self) -> bool {
        self.ocr_config.is_some()
    }

    /// 检查是否配置了多模态 LLM
    pub fn has_vision(&self) -> bool {
        self.vision_config.is_some()
    }

    /// 处理图像
    pub async fn process_image(&self, path: &str) -> Result<ImageContent, ImageError> {
        let start = std::time::Instant::now();

        // 1. 读取图像
        let img_bytes = std::fs::read(path).map_err(|e| ImageError::IoError(e.to_string()))?;
        let img_base64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);

        // 2. 分类图像
        let img_type = self.classify_image(&img_bytes)?;

        // 3. 根据类型选择处理方式
        let content = match img_type {
            ImageType::TextScreenshot => {
                // 纯文字截图 → OCR
                let ocr_text = self.ocr(&img_base64).await?;
                ImageContent {
                    image_type: img_type,
                    ocr_text: Some(ocr_text),
                    description: None,
                    structured_data: None,
                    processing_time_ms: start.elapsed().as_millis() as u64,
                }
            }
            ImageType::Flowchart | ImageType::Architecture => {
                // 流程图/架构图 → 多模态 LLM
                let prompt = match img_type {
                    ImageType::Flowchart => {
                        "请详细描述这个流程图的内容，包括：\
                         1. 流程的起始和结束节点\
                         2. 每个步骤的名称和含义\
                         3. 判断条件和分支逻辑\
                         4. 流程的整体目的"
                    }
                    ImageType::Architecture => {
                        "请详细描述这个架构图的内容，包括：\
                         1. 各个组件/模块的名称\
                         2. 组件之间的关系和依赖\
                         3. 数据流向\
                         4. 整体架构设计思路"
                    }
                    _ => unreachable!(),
                };
                let description = self.vision(&img_base64, prompt).await?;
                ImageContent {
                    image_type: img_type,
                    ocr_text: None,
                    description: Some(description),
                    structured_data: None,
                    processing_time_ms: start.elapsed().as_millis() as u64,
                }
            }
            ImageType::Table => {
                // 表格 → OCR + 结构化提取
                let ocr_text = self.ocr(&img_base64).await?;
                let prompt = format!(
                    "以下是表格图片的 OCR 文本，请提取为结构化的 JSON 格式：\n\n{}\n\n\
                     请返回 JSON 数组，每个元素是一行数据。",
                    ocr_text
                );
                let structured = self.vision(&img_base64, &prompt).await?;
                ImageContent {
                    image_type: img_type,
                    ocr_text: Some(ocr_text),
                    description: None,
                    structured_data: serde_json::from_str(&structured).ok(),
                    processing_time_ms: start.elapsed().as_millis() as u64,
                }
            }
            ImageType::Mixed => {
                // 混合 → 多模态 LLM
                let description = self
                    .vision(&img_base64, "请描述这张图片的内容")
                    .await?;
                ImageContent {
                    image_type: img_type,
                    ocr_text: None,
                    description: Some(description),
                    structured_data: None,
                    processing_time_ms: start.elapsed().as_millis() as u64,
                }
            }
        };

        Ok(content)
    }

    /// 图像分类（基于简单规则）
    fn classify_image(&self, img_bytes: &[u8]) -> Result<ImageType, ImageError> {
        // 尝试加载图片获取尺寸
        let img = image::load_from_memory(img_bytes)
            .map_err(|e| ImageError::FormatError(e.to_string()))?;
        
        let width = img.width();
        let height = img.height();
        let aspect_ratio = width as f64 / height as f64;
        
        // 基于图片尺寸和比例的简单分类
        // 1. 宽高比接近 16:9 或 4:3 的可能是截图
        // 2. 较大的图片可能是架构图
        // 3. 较小的图片可能是图标
        
        if width < 100 || height < 100 {
            // 太小的图片可能是图标
            Ok(ImageType::Mixed)
        } else if aspect_ratio > 1.5 || aspect_ratio < 0.67 {
            // 宽高比差异大，可能是流程图或架构图
            Ok(ImageType::Flowchart)
        } else {
            // 默认为混合类型，后续可以用 LLM 进一步分类
            Ok(ImageType::Mixed)
        }
    }

    /// OCR 文字识别
    async fn ocr(&self, img_base64: &str) -> Result<String, ImageError> {
        let config = self
            .ocr_config
            .as_ref()
            .ok_or(ImageError::OcrNotConfigured)?;

        match config.provider {
            OcrProvider::Baidu => self.ocr_baidu(img_base64, config).await,
            OcrProvider::Tencent => self.ocr_tencent(img_base64, config).await,
            OcrProvider::Tesseract => self.ocr_tesseract(img_base64).await,
        }
    }

    /// 百度 OCR
    async fn ocr_baidu(
        &self,
        img_base64: &str,
        config: &OcrConfig,
    ) -> Result<String, ImageError> {
        // 1. 获取 access_token
        let token = self.get_baidu_token(config).await?;

        // 2. 调用 OCR API
        let response = self
            .client
            .post("https://aip.baidubce.com/rest/2.0/ocr/v1/general_basic")
            .query(&[("access_token", &token)])
            .form(&[("image", img_base64)])
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        // 3. 提取文字
        let words: Vec<String> = result["words_result"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|w| w["words"].as_str().unwrap_or("").to_string())
            .collect();

        Ok(words.join("\n"))
    }

    /// 获取百度 access_token
    async fn get_baidu_token(&self, config: &OcrConfig) -> Result<String, ImageError> {
        let api_key = &config.api_key;
        let secret_key = config
            .secret_key
            .as_ref()
            .ok_or(ImageError::OcrNotConfigured)?;

        let response = self
            .client
            .post("https://aip.baidubce.com/oauth/2.0/token")
            .query(&[
                ("grant_type", "client_credentials"),
                ("client_id", api_key),
                ("client_secret", secret_key),
            ])
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        result["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ImageError::ApiError("获取百度 token 失败".to_string()))
    }

    /// 腾讯 OCR
    async fn ocr_tencent(
        &self,
        img_base64: &str,
        config: &OcrConfig,
    ) -> Result<String, ImageError> {
        let api_key = &config.api_key;

        // 腾讯云 OCR API（通用印刷体识别）
        let response = self
            .client
            .post("https://ocr.tencentcloudapi.com")
            .header("Content-Type", "application/json")
            .header("X-TC-Action", "GeneralBasicOCR")
            .header("X-TC-Version", "2018-11-19")
            .header("X-TC-Region", "ap-guangzhou")
            .header("Authorization", format!("TC3-HMAC-SHA256 Credential={}", api_key))
            .json(&serde_json::json!({
                "ImageBase64": img_base64
            }))
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        // 提取文字
        let words: Vec<String> = result["Response"]["TextDetections"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|w| w["DetectedText"].as_str().unwrap_or("").to_string())
            .collect();

        Ok(words.join("\n"))
    }

    /// 本地 Tesseract OCR
    async fn ocr_tesseract(&self, img_base64: &str) -> Result<String, ImageError> {
        // 将 base64 解码为临时文件
        let img_bytes = base64::engine::general_purpose::STANDARD
            .decode(img_base64)
            .map_err(|e| ImageError::FormatError(e.to_string()))?;

        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("ocr_{}.png", uuid::Uuid::new_v4()));
        std::fs::write(&temp_file, &img_bytes).map_err(|e| ImageError::IoError(e.to_string()))?;

        // 调用 Tesseract 命令行
        let output = tokio::process::Command::new("tesseract")
            .arg(&temp_file)
            .arg("stdout")
            .arg("-l")
            .arg("chi_sim+eng")
            .output()
            .await
            .map_err(|e| ImageError::IoError(format!("Tesseract 执行失败: {}", e)))?;

        // 清理临时文件
        let _ = std::fs::remove_file(&temp_file);

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(ImageError::IoError(format!(
                "Tesseract 错误: {}",
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }

    /// 多模态 LLM 图像理解
    async fn vision(&self, img_base64: &str, prompt: &str) -> Result<String, ImageError> {
        let config = self
            .vision_config
            .as_ref()
            .ok_or(ImageError::VisionNotConfigured)?;

        match config.provider {
            VisionProvider::Gpt4v => self.vision_gpt4v(img_base64, prompt, config).await,
            VisionProvider::QwenVl => self.vision_qwen_vl(img_base64, prompt, config).await,
            VisionProvider::Glm4v => self.vision_glm4v(img_base64, prompt, config).await,
            VisionProvider::Claude => self.vision_claude(img_base64, prompt, config).await,
        }
    }

    /// GPT-4V 图像理解
    async fn vision_gpt4v(
        &self,
        img_base64: &str,
        prompt: &str,
        config: &VisionConfig,
    ) -> Result<String, ImageError> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");

        let response = self
            .client
            .post(format!("{}/chat/completions", base_url))
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(&serde_json::json!({
                "model": "gpt-4-vision-preview",
                "messages": [{
                    "role": "user",
                    "content": [
                        {"type": "text", "text": prompt},
                        {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", img_base64)}}
                    ]
                }],
                "max_tokens": 1000
            }))
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        result["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ImageError::ApiError("GPT-4V 返回为空".to_string()))
    }

    /// 通义千问 VL 图像理解
    async fn vision_qwen_vl(
        &self,
        img_base64: &str,
        prompt: &str,
        config: &VisionConfig,
    ) -> Result<String, ImageError> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or("https://dashscope.aliyuncs.com/compatible-mode/v1");

        let response = self
            .client
            .post(format!("{}/chat/completions", base_url))
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(&serde_json::json!({
                "model": "qwen-vl-plus",
                "messages": [{
                    "role": "user",
                    "content": [
                        {"type": "text", "text": prompt},
                        {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", img_base64)}}
                    ]
                }],
                "max_tokens": 1000
            }))
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        result["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ImageError::ApiError("通义千问 VL 返回为空".to_string()))
    }

    /// 智谱 GLM-4V 图像理解
    async fn vision_glm4v(
        &self,
        img_base64: &str,
        prompt: &str,
        config: &VisionConfig,
    ) -> Result<String, ImageError> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or("https://open.bigmodel.cn/api/paas/v4");

        let response = self
            .client
            .post(format!("{}/chat/completions", base_url))
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(&serde_json::json!({
                "model": "glm-4v",
                "messages": [{
                    "role": "user",
                    "content": [
                        {"type": "text", "text": prompt},
                        {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", img_base64)}}
                    ]
                }],
                "max_tokens": 1000
            }))
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        result["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ImageError::ApiError("智谱 GLM-4V 返回为空".to_string()))
    }

    /// Claude Vision 图像理解
    async fn vision_claude(
        &self,
        img_base64: &str,
        prompt: &str,
        config: &VisionConfig,
    ) -> Result<String, ImageError> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1");

        let response = self
            .client
            .post(format!("{}/messages", base_url))
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&serde_json::json!({
                "model": "claude-3-5-sonnet-20241022",
                "max_tokens": 1000,
                "messages": [{
                    "role": "user",
                    "content": [
                        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": img_base64}},
                        {"type": "text", "text": prompt}
                    ]
                }]
            }))
            .send()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ImageError::ApiError(e.to_string()))?;

        result["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ImageError::ApiError("Claude Vision 返回为空".to_string()))
    }

    /// 计算图像哈希
    pub fn compute_image_hash(path: &str) -> Result<String, ImageError> {
        let bytes = std::fs::read(path).map_err(|e| ImageError::IoError(e.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

// ─── 错误类型 ───

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("API 错误: {0}")]
    ApiError(String),

    #[error("OCR 未配置")]
    OcrNotConfigured,

    #[error("多模态 LLM 未配置")]
    VisionNotConfigured,

    #[error("未实现: {0}")]
    NotImplemented(String),

    #[error("图像格式错误: {0}")]
    FormatError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_type_serialization() {
        let img_type = ImageType::Flowchart;
        let json = serde_json::to_string(&img_type).unwrap();
        assert_eq!(json, "\"Flowchart\"");
    }

    #[test]
    fn test_image_content_serialization() {
        let content = ImageContent {
            image_type: ImageType::TextScreenshot,
            ocr_text: Some("Hello World".to_string()),
            description: None,
            structured_data: None,
            processing_time_ms: 100,
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("Hello World"));
    }

    #[test]
    fn test_compute_image_hash() {
        // 创建临时文件
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.png");
        std::fs::write(&path, b"fake image data").unwrap();

        let hash = ImageProcessor::compute_image_hash(path.to_str().unwrap()).unwrap();
        assert!(!hash.is_empty());
    }
}
