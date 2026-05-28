//! 腾讯实时语音识别 Provider（WebSocket 流式识别）
//!
//! 基于 WebSocket 的实时语音识别，支持边说边出结果。
//! 鉴权方式：HMAC-SHA1 签名 + Base64

use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

use super::asr_provider::*;

type HmacSha1 = Hmac<Sha1>;

/// 腾讯实时语音识别 Provider
pub struct TencentStreamingProvider {
    secret_id: String,
    secret_key: String,
    app_id: u64,
    ws_sender: Option<mpsc::Sender<Message>>,
    result_receiver: Option<mpsc::Receiver<AsrResult>>,
}

impl TencentStreamingProvider {
    pub fn new(secret_id: String, secret_key: String, app_id: u64) -> Self {
        Self {
            secret_id,
            secret_key,
            app_id,
            ws_sender: None,
            result_receiver: None,
        }
    }

    /// 生成 HMAC-SHA1 签名
    fn sign(&self, params: &[(String, String)]) -> String {
        let mut sorted_params = params.to_vec();
        sorted_params.sort_by(|a, b| a.0.cmp(&b.0));

        let sign_url = sorted_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        let mut mac = HmacSha1::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC key error");
        mac.update(sign_url.as_bytes());
        let signature = mac.finalize().into_bytes();
        base64::engine::general_purpose::STANDARD.encode(signature)
    }

    /// 构造 WebSocket URL
    fn build_ws_url(&self, voice_id: &str) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expired = timestamp + 86400; // 24小时过期
        let nonce = rand::random::<u32>();

        let params = vec![
            ("engine_model_type".to_string(), "16k_zh".to_string()),
            ("expired".to_string(), expired.to_string()),
            ("nonce".to_string(), nonce.to_string()),
            ("secretid".to_string(), self.secret_id.clone()),
            ("timestamp".to_string(), timestamp.to_string()),
            ("voice_format".to_string(), "1".to_string()), // PCM
            ("voice_id".to_string(), voice_id.to_string()),
            ("needvad".to_string(), "1".to_string()),
        ];

        let signature = self.sign(&params);
        let encoded_signature = urlencoding::encode(&signature);

        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        format!(
            "wss://asr.cloud.tencent.com/asr/v2/{}?{}&signature={}",
            self.app_id, query, encoded_signature
        )
    }
}

#[async_trait]
impl StreamingAsr for TencentStreamingProvider {
    async fn start_session(&mut self, _config: &AsrConfig) -> Result<(), AsrError> {
        let voice_id = Uuid::new_v4().to_string();
        let url = self.build_ws_url(&voice_id);

        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| AsrError::WebSocketError(format!("连接失败: {}", e)))?;

        let (mut write, mut read) = ws_stream.split();
        let (tx_cmd, mut rx_cmd) = mpsc::channel::<Message>(32);
        let (tx_result, rx_result) = mpsc::channel::<AsrResult>(32);

        self.ws_sender = Some(tx_cmd);
        self.result_receiver = Some(rx_result);

        // 发送音频数据的任务
        tokio::spawn(async move {
            while let Some(msg) = rx_cmd.recv().await {
                if write.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // 接收识别结果的任务
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(resp) = serde_json::from_str::<serde_json::Value>(&text) {
                            if resp["code"].as_i64() == Some(0) {
                                if let Some(result) = resp.get("result") {
                                    let text = result["voice_text_str"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    let slice_type = result["slice_type"].as_i64().unwrap_or(0);

                                    if !text.is_empty() {
                                        let _ = tx_result
                                            .send(AsrResult {
                                                text,
                                                is_final: slice_type == 2,
                                                confidence: 0.8,
                                                processing_time_ms: 0,
                                                segments: None,
                                            })
                                            .await;
                                    }
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        Ok(())
    }

    async fn send_audio(&mut self, data: &[u8]) -> Result<(), AsrError> {
        if let Some(sender) = &self.ws_sender {
            sender
                .send(Message::Binary(data.to_vec()))
                .await
                .map_err(|e| AsrError::WebSocketError(format!("发送失败: {}", e)))?;
        }
        Ok(())
    }

    async fn end_session(&mut self) -> Result<(), AsrError> {
        if let Some(sender) = &self.ws_sender {
            let end_msg = serde_json::json!({"type": "end"});
            sender
                .send(Message::Text(end_msg.to_string()))
                .await
                .map_err(|e| AsrError::WebSocketError(format!("发送结束标识失败: {}", e)))?;
        }
        self.ws_sender = None;
        Ok(())
    }

    async fn receive_result(&mut self) -> Result<Option<AsrResult>, AsrError> {
        if let Some(receiver) = &mut self.result_receiver {
            match tokio::time::timeout(std::time::Duration::from_secs(10), receiver.recv()).await {
                Ok(Some(result)) => Ok(Some(result)),
                Ok(None) => Ok(None),
                Err(_) => Err(AsrError::Timeout),
            }
        } else {
            Ok(None)
        }
    }

    fn provider_name(&self) -> &str {
        "tencent_streaming"
    }
}

#[async_trait]
impl FileAsr for TencentStreamingProvider {
    async fn recognize_file(&self, _audio_data: &[f32], _config: &AsrConfig) -> Result<AsrResult, AsrError> {
        Err(AsrError::UnsupportedOperation(
            "腾讯实时语音识别不支持文件识别，请使用腾讯一句话识别".to_string()
        ))
    }

    async fn recognize_pcm16(&self, _audio_data: &[u8], _config: &AsrConfig) -> Result<AsrResult, AsrError> {
        Err(AsrError::UnsupportedOperation(
            "腾讯实时语音识别不支持文件识别，请使用腾讯一句话识别".to_string()
        ))
    }

    fn provider_name(&self) -> &str {
        "tencent_streaming"
    }

    fn supports_file_recognition(&self) -> bool {
        false
    }
}

#[async_trait]
impl AsrProvider for TencentStreamingProvider {
    fn provider_type(&self) -> AsrProviderType {
        AsrProviderType::TencentStreaming
    }

    fn status_description(&self) -> String {
        if self.ws_sender.is_some() {
            "腾讯实时语音识别会话中".to_string()
        } else {
            "腾讯实时语音识别就绪".to_string()
        }
    }
}
