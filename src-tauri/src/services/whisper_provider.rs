//! Whisper ASR Provider — 适配现有 WhisperService 到 ASR Provider 接口
//!
//! 将现有的 WhisperService 封装为 StreamingAsr + FileAsr 接口，
//! 保持向后兼容的同时支持新的 Provider 架构。

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::asr_provider::*;
use super::whisper_service::WhisperService;

/// Whisper ASR Provider（本地离线识别）
pub struct WhisperAsrProvider {
    service: Arc<Mutex<WhisperService>>,
    model_dir: PathBuf,
    model_size: String,
}

impl WhisperAsrProvider {
    /// 创建新的 Whisper Provider
    pub fn new(model_dir: PathBuf, model_size: String) -> Self {
        Self {
            service: Arc::new(Mutex::new(WhisperService::new())),
            model_dir,
            model_size,
        }
    }

    /// 从现有 WhisperService 创建 Provider
    pub fn from_service(service: Arc<Mutex<WhisperService>>, model_dir: PathBuf, model_size: String) -> Self {
        Self {
            service,
            model_dir,
            model_size,
        }
    }

    /// 加载模型
    pub fn load_model(&self) -> Result<(), AsrError> {
        let mut service = self.service.lock().map_err(|e| AsrError::ModelLoadError(e.to_string()))?;
        service.load_model(&self.model_dir, &self.model_size)
            .map_err(|e| AsrError::ModelLoadError(e))
    }

    /// 检查模型是否已加载
    pub fn is_model_loaded(&self) -> bool {
        self.service.lock().map(|s| s.is_model_loaded()).unwrap_or(false)
    }
}

#[async_trait]
impl StreamingAsr for WhisperAsrProvider {
    async fn start_session(&mut self, _config: &AsrConfig) -> Result<(), AsrError> {
        // Whisper 不需要预热会话，直接开始录音即可
        Ok(())
    }

    async fn send_audio(&mut self, _data: &[u8]) -> Result<(), AsrError> {
        // Whisper 不支持流式发送，音频数据在 end_session 时一次性处理
        Ok(())
    }

    async fn end_session(&mut self) -> Result<(), AsrError> {
        // Whisper 的实际转录在 receive_result 中完成
        Ok(())
    }

    async fn receive_result(&mut self) -> Result<Option<AsrResult>, AsrError> {
        // Whisper 不支持流式结果返回
        Err(AsrError::UnsupportedOperation(
            "Whisper 不支持流式识别，请使用文件识别模式".to_string()
        ))
    }

    fn provider_name(&self) -> &str {
        "whisper"
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}

#[async_trait]
impl FileAsr for WhisperAsrProvider {
    async fn recognize_file(&self, audio_data: &[f32], _config: &AsrConfig) -> Result<AsrResult, AsrError> {
        let service = self.service.lock().map_err(|e| AsrError::TranscriptionError(e.to_string()))?;

        if !service.is_model_loaded() {
            return Err(AsrError::ModelLoadError("Whisper 模型未加载".to_string()));
        }

        let result = service.transcribe(audio_data)
            .map_err(|e| AsrError::TranscriptionError(e))?;

        Ok(AsrResult {
            text: result.text,
            is_final: true,
            confidence: result.confidence,
            processing_time_ms: result.processing_time_ms,
            segments: Some(result.segments.iter().map(|s| AsrSegment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: s.text.clone(),
            }).collect()),
        })
    }

    async fn recognize_pcm16(&self, audio_data: &[u8], config: &AsrConfig) -> Result<AsrResult, AsrError> {
        // 将 PCM 16bit 转换为 f32
        let samples_f32: Vec<f32> = audio_data
            .chunks_exact(2)
            .map(|chunk| {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                sample as f32 / 32768.0
            })
            .collect();

        self.recognize_file(&samples_f32, config).await
    }

    fn provider_name(&self) -> &str {
        "whisper"
    }
}

#[async_trait]
impl AsrProvider for WhisperAsrProvider {
    fn provider_type(&self) -> AsrProviderType {
        AsrProviderType::Whisper
    }

    fn status_description(&self) -> String {
        if self.is_model_loaded() {
            format!("Whisper {} 模型已加载", self.model_size)
        } else {
            "Whisper 模型未加载".to_string()
        }
    }
}
