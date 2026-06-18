//! 多模态探测 — 检测模型是否支持图像输入
//!
//! 提供模块级 free function，不再依赖 LLMProviderManager 实例。
//! 调用方需显式传入 reqwest::Client 和供应商/模型配置。

use base64::Engine;

use super::types::*;
use super::anthropic::{anthropic_messages_url, with_anthropic_headers};

/// 探测指定模型是否支持多模态
pub async fn probe_model_multimodal(
    client: &reqwest::Client,
    provider: &LLMProviderConfig,
    model_name: &str,
    api_key: &str,
) -> bool {
    if provider.protocol != LLMProtocol::Local && api_key.is_empty() {
        return false;
    }

    // 用 1x1 透明图片测试
    let test_img = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    match provider.protocol {
        LLMProtocol::OpenAI => {
            let url = format!(
                "{}/chat/completions",
                provider.base_url.trim_end_matches('/')
            );
            let result = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&serde_json::json!({
                    "model": model_name,
                    "messages": [{"role": "user", "content": [
                        {"type": "text", "text": "test"},
                        {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", test_img)}}
                    ]}],
                    "max_tokens": 1
                }))
                .send()
                .await;

            let mut is_success = false;
            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        is_success = true;
                    } else {
                        let text = resp.text().await.unwrap_or_default();
                        tracing::warn!("OpenAI multimodal probe with Base64 failed for model {}. Status: {}, Response: {}", model_name, status, text);
                    }
                }
                Err(e) => {
                    tracing::warn!("OpenAI multimodal probe with Base64 request failed for model {}. Error: {:?}", model_name, e);
                }
            }

            if is_success {
                return true;
            }

            // 2. 如果 Base64 探测失败，尝试公网图片 URL 探测 (fallback)
            let public_img_url = "https://tauri.app/img/logo-colored.png";
            tracing::info!(
                "Attempting OpenAI multimodal probe with public URL for model {}",
                model_name
            );
            let result_url = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&serde_json::json!({
                    "model": model_name,
                    "messages": [{"role": "user", "content": [
                        {"type": "text", "text": "test"},
                        {"type": "image_url", "image_url": {"url": public_img_url}}
                    ]}],
                    "max_tokens": 1
                }))
                .send()
                .await;

            match result_url {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        is_success = true;
                    } else {
                        let text = resp.text().await.unwrap_or_default();
                        tracing::warn!("OpenAI multimodal probe with public URL failed for model {}. Status: {}, Response: {}", model_name, status, text);
                    }
                }
                Err(e) => {
                    tracing::warn!("OpenAI multimodal probe with public URL request failed for model {}. Error: {:?}", model_name, e);
                }
            }

            if is_success {
                return true;
            }

            // 3. 如果依然失败，尝试本地临时文件路径探测 (适用于可以访问本地路径的本地/内网部署模型)
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(test_img) {
                let temp_path = std::env::temp_dir().join("kingdee_probe_temp.png");
                if std::fs::write(&temp_path, bytes).is_ok() {
                    if let Ok(abs_path) = temp_path.canonicalize() {
                        let file_url = format!(
                            "file:///{}",
                            abs_path.to_string_lossy().replace('\\', "/")
                        );
                        tracing::info!("Attempting OpenAI multimodal probe with local file path for model {}: {}", model_name, file_url);
                        let result_local = client
                            .post(&url)
                            .header("Authorization", format!("Bearer {}", api_key))
                            .json(&serde_json::json!({
                                "model": model_name,
                                "messages": [{"role": "user", "content": [
                                    {"type": "text", "text": "test"},
                                    {"type": "image_url", "image_url": {"url": file_url}}
                                ]}],
                                "max_tokens": 1
                            }))
                            .send()
                            .await;

                        let _ = std::fs::remove_file(&temp_path);

                        match result_local {
                            Ok(resp) => {
                                let status = resp.status();
                                if status.is_success() {
                                    is_success = true;
                                } else {
                                    let text = resp.text().await.unwrap_or_default();
                                    tracing::warn!("OpenAI multimodal probe with local path failed for model {}. Status: {}, Response: {}", model_name, status, text);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("OpenAI multimodal probe with local path request failed for model {}. Error: {:?}", model_name, e);
                            }
                        }
                    }
                }
            }

            is_success
        }
        LLMProtocol::Anthropic => {
            let url = anthropic_messages_url(&provider.base_url);
            let result = with_anthropic_headers(client.post(&url), &url, api_key)
                .json(&serde_json::json!({
                    "model": model_name,
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": "image/png",
                                        "data": test_img
                                    }
                                },
                                {
                                    "type": "text",
                                    "text": "test"
                                }
                            ]
                        }
                    ],
                    "max_tokens": 1
                }))
                .send()
                .await;

            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        true
                    } else {
                        let text = resp.text().await.unwrap_or_default();
                        tracing::warn!("Anthropic multimodal probe failed for model {}. Status: {}, Response: {}", model_name, status, text);
                        false
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Anthropic multimodal probe request failed for model {}. Error: {:?}",
                        model_name,
                        e
                    );
                    false
                }
            }
        }
        LLMProtocol::Local => {
            // 1. 尝试 Ollama 原生 /api/chat 接口
            let ollama_url = format!("{}/api/chat", provider.base_url.trim_end_matches('/'));
            let result = client
                .post(&ollama_url)
                .json(&serde_json::json!({
                    "model": model_name,
                    "messages": [{
                        "role": "user",
                        "content": "test",
                        "images": [test_img]
                    }],
                    "stream": false
                }))
                .send()
                .await;

            let mut ollama_success = false;
            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        ollama_success = true;
                    } else {
                        let text = resp.text().await.unwrap_or_default();
                        tracing::warn!("Local Ollama multimodal probe failed for model {}. Status: {}, Response: {}", model_name, status, text);
                    }
                }
                Err(e) => {
                    tracing::warn!("Local Ollama multimodal probe request failed for model {}. Error: {:?}", model_name, e);
                }
            }

            ollama_success
        }
    }
}

/// 探测供应商的默认模型是否支持多模态
pub async fn probe_multimodal(
    client: &reqwest::Client,
    provider: &LLMProviderConfig,
) -> bool {
    let api_key = provider.get_default_key_value();
    let model_name = provider.get_default_model_name();
    probe_model_multimodal(client, provider, &model_name, &api_key).await
}
