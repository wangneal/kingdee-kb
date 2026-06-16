//! 腾讯云一句话识别（REST API）— 适合短音频（≤60秒）
//!
//! 单一识别入口：`TencentOneShotProvider::recognize_pcm16`。
//! 不再依赖通用 ASR Provider 抽象层（StreamingAsr/FileAsr/AsrProvider），
//! 整个项目目前只有这一个腾讯调用点，直接暴露方法即可。

use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// 腾讯一句话识别 Provider（REST API）
pub struct TencentOneShotProvider {
    secret_id: String,
    secret_key: String,
    client: Client,
}

/// ASR 识别配置（精简：只保留语言）
#[derive(Debug, Clone)]
pub struct TencentAsrConfig {
    pub language: String,
}

impl Default for TencentAsrConfig {
    fn default() -> Self {
        Self {
            language: "zh_cn".to_string(),
        }
    }
}

/// 腾讯 ASR 识别结果
#[derive(Debug, Clone)]
pub struct TencentAsrResult {
    pub text: String,
    pub confidence: f32,
    pub processing_time_ms: u64,
}

/// ASR 错误类型
#[derive(Debug, thiserror::Error)]
pub enum TencentAsrError {
    #[error("网络错误: {0}")]
    NetworkError(String),
    #[error("服务端错误: {code} - {message}")]
    ServerError { code: i32, message: String },
    #[error("时间错误: {0}")]
    TimeError(String),
}

impl TencentOneShotProvider {
    pub fn new(secret_id: String, secret_key: String) -> Self {
        Self {
            secret_id,
            secret_key,
            client: Client::new(),
        }
    }

    /// 识别 PCM 16bit 单声道音频
    pub async fn recognize_pcm16(
        &self,
        audio_data: &[u8],
        config: &TencentAsrConfig,
    ) -> Result<TencentAsrResult, TencentAsrError> {
        let start = std::time::Instant::now();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| TencentAsrError::TimeError(e.to_string()))?
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

        let response = self
            .client
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
            .map_err(|e| TencentAsrError::NetworkError(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| TencentAsrError::NetworkError(e.to_string()))?;

        if !status.is_success() {
            return Err(TencentAsrError::ServerError {
                code: status.as_u16() as i32,
                message: body,
            });
        }

        let resp: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| TencentAsrError::NetworkError(format!("解析响应失败: {}", e)))?;

        if let Some(error) = resp.get("Response").and_then(|r| r.get("Error")) {
            let code = error.get("Code").and_then(|c| c.as_i64()).unwrap_or(-1) as i32;
            let message = error
                .get("Message")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误")
                .to_string();
            return Err(TencentAsrError::ServerError { code, message });
        }

        let text = resp["Response"]["Result"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let processing_time_ms = start.elapsed().as_millis() as u64;

        // 腾讯一句话识别不返回置信度，使用 0.8 作为行业惯例的占位值
        Ok(TencentAsrResult {
            text,
            confidence: 0.8,
            processing_time_ms,
        })
    }

    /// 生成 TC3-HMAC-SHA256 签名
    fn sign(&self, payload: &str, timestamp: u64) -> Result<String, TencentAsrError> {
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
        let secret_date = hmac_sha256(
            format!("TC3{}", self.secret_key).as_bytes(),
            date.as_bytes(),
        );
        let secret_service = hmac_sha256(&secret_date, b"asr");
        let secret_signing = hmac_sha256(&secret_service, b"tc3_request");
        let signature = hex::encode(hmac_sha256(&secret_signing, string_to_sign.as_bytes()));

        Ok(signature)
    }

    /// 构造 Authorization header
    fn authorization(&self, payload: &str, timestamp: u64) -> Result<String, TencentAsrError> {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        let signature = self.sign(payload, timestamp)?;

        Ok(format!(
            "TC3-HMAC-SHA256 Credential={}/{}, SignedHeaders=content-type;host;x-tc-action, Signature={}",
            self.secret_id, date, signature
        ))
    }
}

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
