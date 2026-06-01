//! ASR Provider 抽象层 — 支持本地 Whisper 和在线 ASR 服务（讯飞/腾讯）
//!
//! 设计原则：
//! - 统一的 TranscriptionResult 接口，前端无需关心后端 provider
//! - 流式识别（实时麦克风）和文件识别（录音文件）两种模式
//! - Provider 可插拔，通过配置切换

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ─── 通用类型 ───

/// ASR 识别结果（统一格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrResult {
    /// 识别出的文本
    pub text: String,
    /// 是否为最终结果（流式识别时区分中间/最终）
    pub is_final: bool,
    /// 置信度 (0.0-1.0)
    pub confidence: f32,
    /// 处理耗时（毫秒）
    pub processing_time_ms: u64,
    /// 分段信息（可选，部分 provider 支持）
    pub segments: Option<Vec<AsrSegment>>,
}

/// ASR 分段信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrSegment {
    /// 开始时间（毫秒）
    pub start_ms: u64,
    /// 结束时间（毫秒）
    pub end_ms: u64,
    /// 分段文本
    pub text: String,
}

/// ASR 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrConfig {
    /// 语言代码（zh_cn, en_us 等）
    pub language: String,
    /// 采样率（8000 或 16000）
    pub sample_rate: u32,
    /// 是否启用标点
    pub enable_punctuation: bool,
    /// 是否启用 VAD（语音活动检测）
    pub enable_vad: bool,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            language: "zh_cn".to_string(),
            sample_rate: 16000,
            enable_punctuation: true,
            enable_vad: true,
        }
    }
}

/// ASR Provider 配置（用于创建 provider 实例）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AsrProviderConfig {
    /// 本地 Whisper
    Whisper {
        model_size: String,
        model_dir: String,
    },
    /// 腾讯一句话识别（REST API，适合短音频 ≤60秒）
    TencentOneShot {
        secret_id: String,
        secret_key: String,
        app_id: u64,
    },
    /// 腾讯实时语音识别（WebSocket，适合流式）
    TencentStreaming {
        secret_id: String,
        secret_key: String,
        app_id: u64,
    },
    /// 讯飞语音听写（WebSocket，流式）
    XfyunIat {
        app_id: String,
        api_key: String,
        api_secret: String,
    },
}

/// ASR 错误类型
#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("鉴权失败: {0}")]
    AuthError(String),
    #[error("网络错误: {0}")]
    NetworkError(String),
    #[error("WebSocket 错误: {0}")]
    WebSocketError(String),
    #[error("音频格式错误: {0}")]
    AudioFormatError(String),
    #[error("服务端错误: {code} - {message}")]
    ServerError { code: i32, message: String },
    #[error("模型加载失败: {0}")]
    ModelLoadError(String),
    #[error("转录失败: {0}")]
    TranscriptionError(String),
    #[error("超时")]
    Timeout,
    #[error("不支持的操作: {0}")]
    UnsupportedOperation(String),
}

// ─── Provider Traits ───

/// 流式 ASR Provider（适用于实时麦克风识别）
///
/// 使用流程：start_session → send_audio (多次) → end_session → receive_result (多次)
#[async_trait]
pub trait StreamingAsr: Send + Sync {
    /// 开始识别会话
    async fn start_session(&mut self, config: &AsrConfig) -> Result<(), AsrError>;

    /// 发送音频数据（PCM 16kHz mono f32 或 PCM 16bit）
    async fn send_audio(&mut self, data: &[u8]) -> Result<(), AsrError>;

    /// 结束会话（触发最终识别）
    async fn end_session(&mut self) -> Result<(), AsrError>;

    /// 接收识别结果（流式返回，直到 None 表示结束）
    async fn receive_result(&mut self) -> Result<Option<AsrResult>, AsrError>;

    /// 获取 provider 名称
    fn provider_name(&self) -> &str;

    /// 是否支持流式识别
    fn supports_streaming(&self) -> bool {
        true
    }
}

/// 文件 ASR Provider（适用于录音文件识别）
///
/// 使用流程：recognize_file(audio_data) → 返回完整结果
#[async_trait]
pub trait FileAsr: Send + Sync {
    /// 识别音频文件（PCM 16kHz mono f32）
    async fn recognize_file(
        &self,
        audio_data: &[f32],
        config: &AsrConfig,
    ) -> Result<AsrResult, AsrError>;

    /// 识别音频文件（原始 PCM 16bit）
    async fn recognize_pcm16(
        &self,
        audio_data: &[u8],
        config: &AsrConfig,
    ) -> Result<AsrResult, AsrError>;

    /// 获取 provider 名称
    fn provider_name(&self) -> &str;

    /// 是否支持文件识别
    fn supports_file_recognition(&self) -> bool {
        true
    }
}

/// 统一 ASR Provider（同时支持流式和文件识别）
#[async_trait]
pub trait AsrProvider: StreamingAsr + FileAsr {
    /// 获取 provider 类型
    fn provider_type(&self) -> AsrProviderType;

    /// 获取 provider 状态描述
    fn status_description(&self) -> String;
}

/// ASR Provider 类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsrProviderType {
    /// 本地 Whisper
    Whisper,
    /// 腾讯一句话识别
    TencentOneShot,
    /// 腾讯实时语音识别
    TencentStreaming,
    /// 讯飞语音听写
    XfyunIat,
}

impl AsrProviderType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Whisper => "whisper",
            Self::TencentOneShot => "tencent_oneshot",
            Self::TencentStreaming => "tencent_streaming",
            Self::XfyunIat => "xfyun_iat",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Whisper => "本地 Whisper",
            Self::TencentOneShot => "腾讯一句话识别",
            Self::TencentStreaming => "腾讯实时语音识别",
            Self::XfyunIat => "讯飞语音听写",
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Whisper => "本地离线识别，无需网络，支持中英文",
            Self::TencentOneShot => "短音频识别（≤60秒），REST API，简单易用",
            Self::TencentStreaming => "实时流式识别，WebSocket 长连接，低延迟",
            Self::XfyunIat => "实时流式识别，WebSocket 长连接，支持方言",
        }
    }
}
