//! 讯飞语音听写 Provider（WebSocket 流式识别）
//!
//! 基于 WebSocket 的实时语音识别，支持边说边出结果。
//! 鉴权方式：HMAC-SHA256 签名 + Base64

use async_trait::async_trait;
use base64::Engine;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::asr_provider::*;

type HmacSha256 = Hmac<Sha256>;

/// 讯飞语音听写 Provider
pub struct XfyunIatProvider {
    app_id: String,
    api_key: String,
    api_secret: String,
    ws_sender: Option<mpsc::Sender<Message>>,
    result_receiver: Option<mpsc::Receiver<AsrResult>>,
}

impl XfyunIatProvider {
    pub fn new(app_id: String, api_key: String, api_secret: String) -> Self {
        Self {
            app_id,
            api_key,
            api_secret,
            ws_sender: None,
            result_receiver: None,
        }
    }

    /// 生成 HMAC-SHA256 签名
    fn hmac_sha256_base64(&self, data: &str, key: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("HMAC key error");
        mac.update(data.as_bytes());
        let result = mac.finalize().into_bytes();
        base64::engine::general_purpose::STANDARD.encode(result)
    }

    /// 构造鉴权 URL
    fn build_auth_url(&self) -> String {
        let host = "iat-api.xfyun.cn";
        let path = "/v2/iat";
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();

        // 拼接签名原文
        let signature_origin = format!("host: {}\ndate: {}\nGET {} HTTP/1.1", host, date, path);

        // HMAC-SHA256 签名
        let signature = self.hmac_sha256_base64(&signature_origin, &self.api_secret);

        // 构造 authorization
        let auth_origin = format!(
            r#"api_key="{}", algorithm="hmac-sha256", headers="host date request-line", signature="{}""#,
            self.api_key, signature
        );
        let authorization =
            base64::engine::general_purpose::STANDARD.encode(auth_origin.as_bytes());

        format!(
            "wss://{}{}?authorization={}&date={}&host={}",
            host,
            path,
            urlencoding::encode(&authorization),
            urlencoding::encode(&date),
            host
        )
    }
}

#[async_trait]
impl StreamingAsr for XfyunIatProvider {
    async fn start_session(&mut self, config: &AsrConfig) -> Result<(), AsrError> {
        let url = self.build_auth_url();

        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| AsrError::WebSocketError(format!("连接失败: {}", e)))?;

        let (mut write, mut read) = ws_stream.split();
        let (tx_cmd, mut rx_cmd) = mpsc::channel::<Message>(32);
        let (tx_result, rx_result) = mpsc::channel::<AsrResult>(32);

        self.ws_sender = Some(tx_cmd);
        self.result_receiver = Some(rx_result);

        // 发送首帧（含 common + business 参数）
        let first_frame = serde_json::json!({
            "common": { "app_id": self.app_id },
            "business": {
                "language": "zh_cn",
                "domain": "iat",
                "accent": "mandarin",
                "ptt": if config.enable_punctuation { 1 } else { 0 },
                "dwa": "wpgs",
                "eos": 3000
            },
            "data": {
                "status": 0,
                "format": format!("audio/L16;rate={}", config.sample_rate),
                "encoding": "raw",
                "audio": ""
            }
        });

        write
            .send(Message::Text(first_frame.to_string()))
            .await
            .map_err(|e| AsrError::WebSocketError(format!("发送首帧失败: {}", e)))?;

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
                                if let Some(data) = resp.get("data") {
                                    let status = data["status"].as_i64().unwrap_or(0);
                                    if let Some(result) = data.get("result") {
                                        let ws = result["ws"].as_array();
                                        let mut text = String::new();
                                        if let Some(ws_arr) = ws {
                                            for w in ws_arr {
                                                if let Some(cw) = w["cw"].as_array() {
                                                    for c in cw {
                                                        if let Some(word) = c["w"].as_str() {
                                                            text.push_str(word);
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        if !text.is_empty() {
                                            let _ = tx_result
                                                .send(AsrResult {
                                                    text,
                                                    is_final: status == 2,
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
            // 将 PCM 数据编码为 Base64
            let audio_base64 = base64::engine::general_purpose::STANDARD.encode(data);

            let frame = serde_json::json!({
                "data": {
                    "status": 1,
                    "format": "audio/L16;rate=16000",
                    "encoding": "raw",
                    "audio": audio_base64
                }
            });

            sender
                .send(Message::Text(frame.to_string()))
                .await
                .map_err(|e| AsrError::WebSocketError(format!("发送失败: {}", e)))?;
        }
        Ok(())
    }

    async fn end_session(&mut self) -> Result<(), AsrError> {
        if let Some(sender) = &self.ws_sender {
            let end_frame = serde_json::json!({
                "data": { "status": 2 }
            });
            sender
                .send(Message::Text(end_frame.to_string()))
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
        "xfyun_iat"
    }
}

#[async_trait]
impl FileAsr for XfyunIatProvider {
    async fn recognize_file(
        &self,
        _audio_data: &[f32],
        _config: &AsrConfig,
    ) -> Result<AsrResult, AsrError> {
        Err(AsrError::UnsupportedOperation(
            "讯飞语音听写不支持文件识别，请使用腾讯一句话识别".to_string(),
        ))
    }

    async fn recognize_pcm16(
        &self,
        _audio_data: &[u8],
        _config: &AsrConfig,
    ) -> Result<AsrResult, AsrError> {
        Err(AsrError::UnsupportedOperation(
            "讯飞语音听写不支持文件识别，请使用腾讯一句话识别".to_string(),
        ))
    }

    fn provider_name(&self) -> &str {
        "xfyun_iat"
    }

    fn supports_file_recognition(&self) -> bool {
        false
    }
}

#[async_trait]
impl AsrProvider for XfyunIatProvider {
    fn provider_type(&self) -> AsrProviderType {
        AsrProviderType::XfyunIat
    }

    fn status_description(&self) -> String {
        if self.ws_sender.is_some() {
            "讯飞语音听写会话中".to_string()
        } else {
            "讯飞语音听写就绪".to_string()
        }
    }
}
