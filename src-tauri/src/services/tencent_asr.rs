//! 腾讯云 ASR Provider — 支持一句话识别（REST）和实时语音识别（WebSocket）
//!
//! 一句话识别：适合短音频（≤60秒），简单易用
//! 实时语音识别：适合流式识别，低延迟

use async_trait::async_trait;
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha1::Sha1;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

use super::asr_provider::*;

type HmacSha1 = Hmac<Sha1>;
type HmacSha256 = Hmac<Sha256>;

/// 腾讯一句话识别 Provider（REST API）
pub struct TencentOneShotProvider {
    secret_id: String,
    secret_key: String,
    app_id: u64,
    client: Client,
}

impl TencentOneShotProvider {
    pub fn new(secret_id: String, secret_key: String, app_id: u64) -> Self {
        Self {
            secret_id,
            secret_key,
            app_id,
            client: Client::new(),
        }
    }

    /// 生成 TC3-HMAC-SHA256 签名
    fn sign(&self, payload: &str, timestamp: u64) -> Result<String, AsrError> {
        let date = Utc::now().format("%Y-%m-%d").to_string();

        // Step 1: 拼接规范请求串
        let canonical_request = format!(
            "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:asr.tencentcloudapi.com\nx-tc-action:sentencerecognition\n\ncontent-type;host;x-tc-action\n{}",
            sha256_hex(payload)
        );

        // Step 2: 拼接待签名字符串
        let credential_scope = format!("{}/asr/tc3_request", date);
        let string_to_sign = format!(
            "TC3-HMAC-SHA256\n{}\n{}\n{}",
            timestamp,
            credential_scope,
            sha256_hex(&canonical_request)
        );

        // Step 3: 计算签名
        let secret_date = hmac_sha256(format!("TC3{}", self.secret_key).as_bytes(), date.as_bytes());
        let secret_service = hmac_sha256(&secret_date, b"asr");
        let secret_signing = hmac_sha256(&secret_service, b"tc3_request");
        let signature = hex::encode(hmac_sha256(&secret_signing, string_to_sign.as_bytes()));

        Ok(signature)
    }

    /// 构造 Authorization header
    fn authorization(&self, payload: &str, timestamp: u64) -> Result<String, AsrError> {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        let signature = self.sign(payload, timestamp)?;

        Ok(format!(
            "TC3-HMAC-SHA256 Credential={}/{}, SignedHeaders=content-type;host;x-tc-action, Signature={}",
            self.secret_id, date, signature
        ))
    }
}

#[async_trait]
impl FileAsr for TencentOneShotProvider {
    async fn recognize_file(&self, audio_data: &[f32], config: &AsrConfig) -> Result<AsrResult, AsrError> {
        // 将 f32 转换为 PCM 16bit
        let pcm16: Vec<u8> = audio_data
            .iter()
            .map(|&s| {
                let sample = (s * 32768.0).clamp(-32768.0, 32767.0) as i16;
                sample.to_le_bytes().to_vec()
            })
            .flatten()
            .collect();

        self.recognize_pcm16(&pcm16, config).await
    }

    async fn recognize_pcm16(&self, audio_data: &[u8], config: &AsrConfig) -> Result<AsrResult, AsrError> {
        let start = std::time::Instant::now();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AsrError::NetworkError(e.to_string()))?
            .as_secs();

        let audio_base64 = base64::engine::general_purpose::STANDARD.encode(audio_data);

        let payload = serde_json::json!({
            "Action": "SentenceRecognition",
            "Version": "2019-06-14",
            "ProjectId": 0,
            "SubServiceType": 2,
            "EngSerViceType": if config.language.starts_with("zh") { "16k_zh" } else { "16k_en" },
            "SourceType": 1,
            "VoiceFormat": "pcm",
            "Data": audio_base64,
            "DataLen": audio_data.len(),
        });

        let payload_str = payload.to_string();
        let authorization = self.authorization(&payload_str, timestamp)?;

        let response = self.client
            .post("https://asr.tencentcloudapi.com")
            .header("Content-Type", "application/json; charset=utf-8")
            .header("Host", "asr.tencentcloudapi.com")
            .header("X-TC-Action", "SentenceRecognition")
            .header("X-TC-Version", "2019-06-14")
            .header("X-TC-Timestamp", timestamp.to_string())
            .header("Authorization", authorization)
            .body(payload_str)
            .send()
            .await
            .map_err(|e| AsrError::NetworkError(e.to_string()))?;

        let status = response.status();
        let body = response.text().await.map_err(|e| AsrError::NetworkError(e.to_string()))?;

        if !status.is_success() {
            return Err(AsrError::ServerError {
                code: status.as_u16() as i32,
                message: body,
            });
        }

        let resp: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| AsrError::NetworkError(format!("解析响应失败: {}", e)))?;

        if let Some(error) = resp.get("Response").and_then(|r| r.get("Error")) {
            let code = error.get("Code").and_then(|c| c.as_i64()).unwrap_or(-1) as i32;
            let message = error.get("Message").and_then(|m| m.as_str()).unwrap_or("未知错误").to_string();
            return Err(AsrError::ServerError { code, message });
        }

        let text = resp["Response"]["Result"].as_str().unwrap_or("").to_string();
        let processing_time_ms = start.elapsed().as_millis() as u64;

        Ok(AsrResult {
            text,
            is_final: true,
            confidence: 0.8, // 腾讯不返回置信度
            processing_time_ms,
            segments: None,
        })
    }

    fn provider_name(&self) -> &str {
        "tencent_oneshot"
    }
}

#[async_trait]
impl StreamingAsr for TencentOneShotProvider {
    async fn start_session(&mut self, _config: &AsrConfig) -> Result<(), AsrError> {
        Err(AsrError::UnsupportedOperation(
            "腾讯一句话识别不支持流式识别，请使用腾讯实时语音识别".to_string()
        ))
    }

    async fn send_audio(&mut self, _data: &[u8]) -> Result<(), AsrError> {
        Err(AsrError::UnsupportedOperation("不支持流式识别".to_string()))
    }

    async fn end_session(&mut self) -> Result<(), AsrError> {
        Err(AsrError::UnsupportedOperation("不支持流式识别".to_string()))
    }

    async fn receive_result(&mut self) -> Result<Option<AsrResult>, AsrError> {
        Err(AsrError::UnsupportedOperation("不支持流式识别".to_string()))
    }

    fn provider_name(&self) -> &str {
        "tencent_oneshot"
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}

#[async_trait]
impl AsrProvider for TencentOneShotProvider {
    fn provider_type(&self) -> AsrProviderType {
        AsrProviderType::TencentOneShot
    }

    fn status_description(&self) -> String {
        "腾讯一句话识别就绪".to_string()
    }
}

// ─── 辅助函数 ───

fn sha256_hex(data: &str) -> String {
    use sha2::Digest;
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key error");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}
