use crate::services::llm_service::{LLMConfig, LLMProvider};
use bytes::Bytes;
use futures::StreamExt;
use rig::http_client::{self, HttpClientExt, MultipartForm, Request, Response};

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

#[derive(Clone, Debug, Default)]
pub struct CompatReqwestClient {
    inner: reqwest::Client,
}

fn needs_relaxed_tls(base_url: &str) -> bool {
    let base_url = base_url.to_ascii_lowercase();
    base_url.starts_with("https://maas.gd.chinamobile.com")
}

fn custom_http_client(base_url: &str) -> Result<CompatReqwestClient, String> {
    let mut builder = reqwest::Client::builder();

    if needs_relaxed_tls(base_url) {
        builder = builder
            .danger_accept_invalid_certs(true)
            .http1_only()
            .no_proxy();
    }

    builder
        .build()
        .map(|inner| CompatReqwestClient { inner })
        .map_err(|e| format!("构建兼容 HTTP client 失败: {}", e))
}

fn instance_error<E>(error: E) -> http_client::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    http_client::Error::Instance(error.into())
}

impl CompatReqwestClient {
    async fn send_request<T, U>(
        &self,
        req: Request<T>,
    ) -> http_client::Result<Response<http_client::LazyBody<U>>>
    where
        T: Into<Bytes>,
        U: From<Bytes> + Send + 'static,
    {
        let (parts, body) = req.into_parts();
        let req = self
            .inner
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body.into());

        let response = req.send().await.map_err(instance_error)?;
        if !response.status().is_success() {
            let status = response.status();
            let message = response.text().await.unwrap_or_default();
            return Err(http_client::Error::InvalidStatusCodeWithMessage(
                status, message,
            ));
        }

        let mut res = Response::builder()
            .status(response.status())
            .version(response.version());

        if let Some(headers) = res.headers_mut() {
            *headers = response.headers().clone();
        }

        let body: http_client::LazyBody<U> = Box::pin(async {
            let bytes = response
                .bytes()
                .await
                .map_err(instance_error)?;
            Ok(U::from(bytes))
        });

        res.body(body).map_err(http_client::Error::Protocol)
    }
}

impl HttpClientExt for CompatReqwestClient {
    fn send<T, U>(
        &self,
        req: Request<T>,
    ) -> impl std::future::Future<Output = http_client::Result<Response<http_client::LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes>,
        T: Send,
        U: From<Bytes>,
        U: Send + 'static,
    {
        let client = self.inner.clone();
        let (parts, body) = req.into_parts();
        let body: Bytes = body.into();
        let req = client
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body);

        async move {
            let response = req.send().await.map_err(instance_error)?;
            if !response.status().is_success() {
                let status = response.status();
                let message = response.text().await.unwrap_or_default();
                return Err(http_client::Error::InvalidStatusCodeWithMessage(
                    status, message,
                ));
            }

            let mut res = Response::builder()
                .status(response.status())
                .version(response.version());

            if let Some(headers) = res.headers_mut() {
                *headers = response.headers().clone();
            }

            let body: http_client::LazyBody<U> = Box::pin(async {
                let bytes = response.bytes().await.map_err(instance_error)?;
                Ok(U::from(bytes))
            });

            res.body(body).map_err(http_client::Error::Protocol)
        }
    }

    fn send_multipart<U>(
        &self,
        req: Request<MultipartForm>,
    ) -> impl std::future::Future<Output = http_client::Result<Response<http_client::LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes>,
        U: Send + 'static,
    {
        let client = self.clone();
        async move {
            let (mut parts, form) = req.into_parts();
            let (boundary, body) = form.encode();
            parts.headers.insert(
                "content-type",
                rig::http_client::HeaderValue::from_str(&format!(
                    "multipart/form-data; boundary={boundary}"
                ))?,
            );
            client.send_request(Request::from_parts(parts, body)).await
        }
    }

    fn send_streaming<T>(
        &self,
        req: Request<T>,
    ) -> impl std::future::Future<Output = http_client::Result<http_client::StreamingResponse>> + Send
    where
        T: Into<Bytes>,
    {
        let client = self.clone();
        let (parts, body) = req.into_parts();
        let body: Bytes = body.into();
        async move {
            let req = client
                .inner
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .body(body)
                .build()
                .map_err(instance_error)?;

            let response = client
                .inner
                .execute(req)
                .await
                .map_err(instance_error)?;
            if !response.status().is_success() {
                let status = response.status();
                let message = response.text().await.unwrap_or_default();
                return Err(http_client::Error::InvalidStatusCodeWithMessage(
                    status, message,
                ));
            }

            let mut res = Response::builder()
                .status(response.status())
                .version(response.version());

            if let Some(headers) = res.headers_mut() {
                *headers = response.headers().clone();
            }

            let stream: rig::http_client::sse::BoxedStream = Box::pin(
                response
                    .bytes_stream()
                    .map(|chunk| chunk.map_err(instance_error)),
            );

            res.body(stream).map_err(http_client::Error::Protocol)
        }
    }
}

pub fn build_openai_client(
    config: &LLMConfig,
) -> Result<rig::providers::openai::Client<CompatReqwestClient>, String> {
    if config.api_key.is_empty() && config.provider != LLMProvider::Local {
        return Err("API key 为空，无法构建 rig OpenAI client".to_string());
    }

    let api_key = if config.api_key.is_empty() {
        "unused".to_string()
    } else {
        config.api_key.clone()
    };

    let mut builder = rig::providers::openai::Client::builder()
        .api_key(api_key)
        .http_client(custom_http_client(&config.base_url)?);

    if config.base_url != DEFAULT_OPENAI_BASE_URL && !config.base_url.is_empty() {
        builder = builder.base_url(config.base_url.trim_end_matches('/'));
    }

    builder
        .build()
        .map_err(|e| format!("构建 rig OpenAI client 失败: {}", e))
}

pub fn build_anthropic_client(
    config: &LLMConfig,
) -> Result<rig::providers::anthropic::Client<CompatReqwestClient>, String> {
    if config.api_key.is_empty() {
        return Err("API key 为空，无法构建 rig Anthropic client".to_string());
    }

    let mut builder = rig::providers::anthropic::Client::builder()
        .api_key(&config.api_key)
        .http_client(custom_http_client(&config.base_url)?);

    if config.base_url != DEFAULT_ANTHROPIC_BASE_URL && !config.base_url.is_empty() {
        builder = builder.base_url(config.base_url.trim_end_matches('/'));
    }

    builder
        .build()
        .map_err(|e| format!("构建 rig Anthropic client 失败: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use rig::client::CompletionClient;
    use rig::streaming::StreamingPrompt;
    use serde::Deserialize;

    use crate::services::rig_tool::all_rig_tools;

    #[derive(Debug, Deserialize)]
    struct SavedConfig {
        provider: LLMProvider,
        api_key: String,
        base_url: String,
        model: String,
        max_tokens: u32,
        temperature: f32,
    }

    fn load_saved_config() -> Option<LLMConfig> {
        let path = dirs::home_dir()?.join(".kingdee-kb").join("config.json");
        let data = std::fs::read_to_string(path).ok()?;
        let saved: SavedConfig = serde_json::from_str(&data).ok()?;
        Some(LLMConfig {
            provider: saved.provider,
            api_key: saved.api_key,
            base_url: saved.base_url,
            model: saved.model,
            max_tokens: saved.max_tokens,
            temperature: saved.temperature,
        })
    }

    #[test]
    #[ignore = "Live MaaS endpoint probe; reads ~/.kingdee-kb/config.json"]
    fn probe_configured_maas_with_rig_compat_client() {
        tauri::async_runtime::block_on(async {
            let config = load_saved_config().expect("missing ~/.kingdee-kb/config.json");
            assert!(
                needs_relaxed_tls(&config.base_url),
                "configured endpoint is not MaaS"
            );

            let client = build_openai_client(&config)
                .expect("build OpenAI client")
                .completions_api();

            let mut stream = client
                .agent(&config.model)
                .max_tokens(32)
                .temperature(0.0)
                .build()
                .stream_prompt("Hi")
                .await;

            let mut saw_any = false;
            while let Some(item) = stream.next().await {
                item.expect("rig stream item");
                saw_any = true;
            }

            assert!(saw_any, "rig stream returned no items");
        });
    }

    #[test]
    #[ignore = "Live MaaS endpoint probe with tool schemas; reads ~/.kingdee-kb/config.json"]
    fn probe_configured_maas_with_rig_tools() {
        tauri::async_runtime::block_on(async {
            let config = load_saved_config().expect("missing ~/.kingdee-kb/config.json");
            assert!(
                needs_relaxed_tls(&config.base_url),
                "configured endpoint is not MaaS"
            );

            let client = build_openai_client(&config)
                .expect("build OpenAI client")
                .completions_api();

            let mut stream = client
                .agent(&config.model)
                .tools(all_rig_tools())
                .max_tokens(32)
                .temperature(0.0)
                .build()
                .stream_prompt("Hi")
                .await;

            let mut saw_any = false;
            while let Some(item) = stream.next().await {
                item.expect("rig stream item");
                saw_any = true;
            }

            assert!(saw_any, "rig stream returned no items");
        });
    }
}
