//! 多模态探测：探测当前 LLM 是否支持多模态图片输入

use super::ImageProcessor;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::timeout;

/// 探测当前 LLM 是否支持多模态（带 15s 超时）
pub async fn probe_multimodal(processor: &ImageProcessor) -> bool {
    if processor.probed.load(Ordering::Relaxed) {
        return processor.llm_multimodal.load(Ordering::Relaxed);
    }

    if processor.requires_api_key() {
        processor.probed.store(true, Ordering::Relaxed);
        return false;
    }

    let probe_result = timeout(Duration::from_secs(15), probe_multimodal_inner(processor)).await;
    match probe_result {
        Ok(is_multimodal) => {
            processor.llm_multimodal.store(is_multimodal, Ordering::Relaxed);
            processor.probed.store(true, Ordering::Relaxed);
            is_multimodal
        }
        Err(_) => {
            tracing::warn!("多模态探测超时（15s）");
            processor.probed.store(true, Ordering::Relaxed);
            false
        }
    }
}

/// 内部多模态探测逻辑（被 timeout 包裹）
async fn probe_multimodal_inner(processor: &ImageProcessor) -> bool {
    // 用 1x1 透明图片测试
    let test_img = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    let protocol = processor
        .protocol
        .as_ref()
        .unwrap_or(&crate::services::llm_providers::LLMProtocol::OpenAI);

    let result = match protocol {
        // Anthropic Messages API 探测
        crate::services::llm_providers::LLMProtocol::Anthropic => {
            let url =
                crate::services::llm_providers::anthropic_messages_url(&processor.llm_base_url);
            let mut req = processor.client
                .post(&url)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&serde_json::json!({
                    "model": processor.llm_model,
                    "max_tokens": 1,
                    "messages": [{"role": "user", "content": [
                        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": test_img}},
                        {"type": "text", "text": "test"}
                    ]}]
                }));
            if !processor.llm_api_key.is_empty() {
                req = req.header("x-api-key", &processor.llm_api_key);
            }
            req.send().await
        }
        // Ollama 原生协议探测
        crate::services::llm_providers::LLMProtocol::Local => {
            processor.client
                .post(format!(
                    "{}/api/chat",
                    processor.llm_base_url.trim_end_matches('/')
                ))
                .json(&serde_json::json!({
                    "model": processor.llm_model,
                    "messages": [{
                        "role": "user",
                        "content": "test",
                        "images": [test_img]
                    }],
                    "stream": false
                }))
                .send()
                .await
        }
        // OpenAI 兼容协议探测
        crate::services::llm_providers::LLMProtocol::OpenAI => {
            let mut req = processor.client
                .post(format!("{}/chat/completions", processor.llm_base_url))
                .json(&serde_json::json!({
                    "model": processor.llm_model,
                    "messages": [{"role": "user", "content": [
                        {"type": "text", "text": "test"},
                        {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", test_img)}}
                    ]}],
                    "max_tokens": 1
                }));
            if !processor.llm_api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", processor.llm_api_key));
            }
            req.send().await
        }
    };

    match result {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                // 非 2xx 状态码 → 不支持多模态
                tracing::info!("多模态探测失败, HTTP {}, 模型 {}", status, processor.llm_model);
                false
            } else {
                // 2xx 状态码还需检查响应体：某些 API 代理/网关会返回 200 但 body 含错误
                match resp.text().await {
                    Ok(body) => {
                        // 尝试解析为 JSON，检查是否有有效响应
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                            // OpenAI 格式为 choices，Anthropic 格式为 content，Ollama 格式为 message
                            if json["choices"].is_array()
                                || json["content"].is_array()
                                || json["message"].is_object()
                            {
                                true
                            } else if json["error"].is_object() {
                                // API 在 body 中返回错误
                                tracing::info!(
                                    "多模态探测收到 200 但 body 含错误, 模型 {}: {:?}",
                                    processor.llm_model,
                                    json["error"]
                                );
                                false
                            } else {
                                // 有响应但格式不标准，保守认为支持
                                tracing::info!(
                                    "多模态探测收到非标准响应格式, 模型 {}, 假定支持多模态",
                                    processor.llm_model
                                );
                                true
                            }
                        } else {
                            // 非 JSON 响应，保守认为支持（某些本地模型返回非标准格式）
                            tracing::info!(
                                "多模态探测收到非 JSON 响应, 模型 {}, 假定支持多模态",
                                processor.llm_model
                            );
                            true
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "多模态探测读取响应体失败, 模型 {}: {:?}",
                            processor.llm_model,
                            e
                        );
                        false
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("多模态探测 HTTP 请求失败, 模型 {}: {:?}", processor.llm_model, e);
            false
        }
    }
}
