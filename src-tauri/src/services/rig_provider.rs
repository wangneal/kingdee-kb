use crate::services::llm_providers::{LLMProviderConfig, LLMProtocol};
use bytes::Bytes;
use futures::StreamExt;
use rig_core::http_client::{self, HttpClientExt, MultipartForm, Request, Response};

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
            let bytes = response.bytes().await.map_err(instance_error)?;
            Ok(U::from(bytes))
        });

        res.body(body).map_err(http_client::Error::Protocol)
    }
}

impl HttpClientExt for CompatReqwestClient {
    fn send<T, U>(
        &self,
        req: Request<T>,
    ) -> impl std::future::Future<Output = http_client::Result<Response<http_client::LazyBody<U>>>>
           + Send
           + 'static
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
    ) -> impl std::future::Future<Output = http_client::Result<Response<http_client::LazyBody<U>>>>
           + Send
           + 'static
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
                rig_core::http_client::HeaderValue::from_str(&format!(
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

            let response = client.inner.execute(req).await.map_err(instance_error)?;
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

            let stream: rig_core::http_client::sse::BoxedStream = Box::pin(
                response
                    .bytes_stream()
                    .map(|chunk| chunk.map_err(instance_error)),
            );

            res.body(stream).map_err(http_client::Error::Protocol)
        }
    }
}

pub fn build_openai_client(
    config: &LLMProviderConfig,
) -> Result<rig_core::providers::openai::Client<CompatReqwestClient>, String> {
    let api_key = config.get_default_key_value();
    if api_key.is_empty() && config.protocol != LLMProtocol::Local {
        return Err("API key 为空，无法构建 rig OpenAI client".to_string());
    }

    let api_key = if api_key.is_empty() {
        "unused".to_string()
    } else {
        api_key
    };

    let mut builder = rig_core::providers::openai::Client::builder()
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
    config: &LLMProviderConfig,
) -> Result<rig_core::providers::anthropic::Client<CompatReqwestClient>, String> {
    let api_key = config.get_default_key_value();
    if api_key.is_empty() {
        return Err("API key 为空，无法构建 rig Anthropic client".to_string());
    }

    let mut builder = rig_core::providers::anthropic::Client::builder()
        .api_key(&api_key)
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
    use rig_core::client::CompletionClient;
    use rig_core::streaming::StreamingPrompt;
    use std::sync::Arc;

    use crate::services::bm25_service::BM25Service;
    use crate::services::embedding::EmbeddingService;
    use crate::services::llm_providers::{LLMProviderConfig, LLMProtocol, LLMProviderManager};
    use crate::services::llm_service::LLMService;
    use crate::services::metadata::MetadataStore;
    use crate::services::product_store::ProductStore;
    use crate::services::rig_tool::all_rig_tools;
    use crate::services::risk_control::RiskControlStore;
    use crate::services::skill_manager::SkillManager;
    use crate::services::vector_index::VectorIndex;

    fn load_saved_config() -> Option<LLMProviderConfig> {
        let data_dir = dirs::home_dir()?.join(".kingdee-kb");
        let manager = LLMProviderManager::new(&data_dir);
        manager.get_default_provider().cloned()
    }

    fn test_tool_deps(
        root: &std::path::Path,
    ) -> (
        Arc<std::sync::Mutex<EmbeddingService>>,
        Arc<std::sync::Mutex<VectorIndex>>,
        Arc<std::sync::Mutex<BM25Service>>,
        Arc<std::sync::Mutex<MetadataStore>>,
        Arc<std::sync::Mutex<ProductStore>>,
        Arc<tokio::sync::Mutex<RiskControlStore>>,
    ) {
        (
            Arc::new(std::sync::Mutex::new(EmbeddingService::empty())),
            Arc::new(std::sync::Mutex::new(
                VectorIndex::new(root.join("vector")).expect("vector index"),
            )),
            Arc::new(std::sync::Mutex::new(
                BM25Service::new(root.join("bm25")).expect("bm25 index"),
            )),
            Arc::new(std::sync::Mutex::new(
                MetadataStore::new(root.join("metadata.db")).expect("metadata store"),
            )),
            Arc::new(std::sync::Mutex::new(
                ProductStore::new(root.join("products.db")).expect("product store"),
            )),
            Arc::new(tokio::sync::Mutex::new(
                RiskControlStore::new(&root.join("metadata.db")).expect("risk control store"),
            )),
        )
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
                .agent(&config.get_default_model_name())
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

            let tmp = tempfile::tempdir().expect("tempdir");
            let (embedding, vector_index, bm25, metadata, products, risk_store) =
                test_tool_deps(tmp.path());
            let providers = Arc::new(std::sync::Mutex::new(
                LLMProviderManager::new(&tmp.path().to_path_buf())
            ));
            let llm = LLMService::new(providers);
            let skill_manager = Arc::new(std::sync::Mutex::new(SkillManager::new(
                tmp.path().join("skills"),
            )));

            let mut stream = client
                .agent(&config.get_default_model_name())
                .tools(all_rig_tools(
                    None,
                    tmp.path().to_path_buf(),
                    llm,
                    embedding,
                    vector_index,
                    bm25,
                    metadata,
                    products,
                    risk_store,
                    skill_manager,
                    None,
                ))
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
