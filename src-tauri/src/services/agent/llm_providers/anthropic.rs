//! Anthropic 协议辅助函数
//!
//! 处理 Anthropic Messages API 的 URL 构造和请求头设置。

fn is_official_anthropic_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.eq_ignore_ascii_case("api.anthropic.com"))
        })
        .unwrap_or(false)
}

/// 构造 Anthropic Messages API 的完整 URL
///
/// base_url 可能是 `https://api.anthropic.com/v1` 或 `https://api.anthropic.com`，
/// 需要归一化避免拼接出 `/v1/v1/messages`。
/// 此函数去除尾部的 `/v1` 后重新拼接，保证结果一致。
pub fn anthropic_messages_url(base_url: &str) -> String {
    let normalized = base_url.trim_end_matches('/');
    let normalized = normalized.trim_end_matches("/v1");
    format!("{}/v1/messages", normalized)
}

pub fn with_anthropic_headers(
    request: reqwest::RequestBuilder,
    url: &str,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let request = request.header("x-api-key", api_key);
    let request = if is_official_anthropic_url(url) {
        request
    } else {
        request.header("Authorization", format!("Bearer {}", api_key))
    };

    request
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-dangerous-direct-browser-access", "true")
        .header("Content-Type", "application/json")
}
