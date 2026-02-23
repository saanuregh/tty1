use std::time::Duration;

use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, HeaderMap, HeaderValue, UPGRADE_INSECURE_REQUESTS,
    USER_AGENT,
};
use reqwest_middleware::ClientBuilder;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use reqwest_tracing::TracingMiddleware;
use tracing::info;

pub type Client = reqwest_middleware::ClientWithMiddleware;

const REQUEST_TIMEOUT_SECS: u64 = 10;
const MAX_RETRIES: u32 = 3;

/// Chrome 145 on Windows 10 — the single most common browser/OS combination.
const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36";

pub fn build_client() -> Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .cookie_store(true)
        .default_headers(chrome_headers());

    if let Some(url) = proxy_url() {
        info!("using proxy: {}", mask_credentials(&url));
        let proxy = reqwest::Proxy::all(&url)
            .unwrap_or_else(|e| panic!("invalid proxy URL {}: {e}", mask_credentials(&url)));
        builder = builder.proxy(proxy);
    }

    let raw_client = builder.build().expect("failed to build HTTP client");

    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(MAX_RETRIES);

    ClientBuilder::new(raw_client)
        .with(TracingMiddleware::default())
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}

/// Emulates Chrome 145 on Windows — header names and values match a real
/// Chrome navigation request captured from DevTools.
fn chrome_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(USER_AGENT, HeaderValue::from_static(CHROME_UA));
    h.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"),
    );
    h.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    h.insert(CACHE_CONTROL, HeaderValue::from_static("max-age=0"));
    h.insert(UPGRADE_INSECURE_REQUESTS, HeaderValue::from_static("1"));
    // Client Hints
    h.insert(
        "Sec-Ch-Ua",
        HeaderValue::from_static(
            "\"Chromium\";v=\"145\", \"Not_A Brand\";v=\"24\", \"Google Chrome\";v=\"145\"",
        ),
    );
    h.insert("Sec-Ch-Ua-Mobile", HeaderValue::from_static("?0"));
    h.insert(
        "Sec-Ch-Ua-Platform",
        HeaderValue::from_static("\"Windows\""),
    );
    // Fetch metadata
    h.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
    h.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
    h.insert("Sec-Fetch-Site", HeaderValue::from_static("none"));
    h.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
    h.insert("Priority", HeaderValue::from_static("u=0, i"));
    h
}

fn proxy_url() -> Option<String> {
    std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("ALL_PROXY"))
        .ok()
        .filter(|s| !s.is_empty())
}

/// Masks credentials in a proxy URL for safe logging.
/// `socks5://user:pass@host:1080` → `socks5://***@host:1080`
fn mask_credentials(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let Some((_, host)) = rest.split_once('@') else {
        return url.to_string();
    };
    format!("{scheme}://***@{host}")
}
