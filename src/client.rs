use std::time::Duration;

use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, UPGRADE_INSECURE_REQUESTS, USER_AGENT,
};
use reqwest_middleware::ClientBuilder;
use reqwest_tracing::TracingMiddleware;
use tracing::info;

use crate::config;

pub type Client = reqwest_middleware::ClientWithMiddleware;

/// Chrome 152 on Windows 11 — the single most common browser/OS combination.
const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36";

/// Reddit blocks datacenter ranges, so it may need a proxy. Hacker News and
/// GitHub do not, and routing them through one only adds latency and risk.
pub struct Clients {
    pub direct: Client,
    pub reddit: Client,
}

pub fn build_clients() -> Clients {
    // no_proxy() is load-bearing: reqwest applies HTTPS_PROXY/ALL_PROXY from the
    // environment unless it is switched off (auto_sys_proxy defaults to true).
    let direct = wrap(
        base_builder()
            .no_proxy()
            .build()
            .expect("failed to build HTTP client"),
    );

    let reddit = match config::reddit_proxy() {
        Some(url) => {
            info!("reddit proxy: {}", mask_credentials(&url));
            let proxy = reqwest::Proxy::all(&url)
                .unwrap_or_else(|e| panic!("invalid REDDIT_PROXY {}: {e}", mask_credentials(&url)));
            // .proxy() also clears auto_sys_proxy, so the environment is ignored here too.
            wrap(
                base_builder()
                    .proxy(proxy)
                    .build()
                    .expect("failed to build Reddit HTTP client"),
            )
        }
        None => {
            info!("reddit proxy: none");
            direct.clone()
        }
    };

    Clients { direct, reddit }
}

fn base_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(config::REQUEST_TIMEOUT_SECS))
        .cookie_store(true)
        .default_headers(chrome_headers())
}

fn wrap(client: reqwest::Client) -> Client {
    ClientBuilder::new(client)
        .with(TracingMiddleware::default())
        .build()
}

/// Emulates Chrome 152 on Windows — header names and values match a real
/// Chrome navigation request captured from DevTools.
fn chrome_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(USER_AGENT, HeaderValue::from_static(CHROME_UA));
    h.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"),
    );
    h.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    h.insert(UPGRADE_INSECURE_REQUESTS, HeaderValue::from_static("1"));
    // Client Hints
    h.insert(
        "Sec-Ch-Ua",
        HeaderValue::from_static(
            "\"Google Chrome\";v=\"152\", \"Chromium\";v=\"152\", \"Not)A;Brand\";v=\"24\"",
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
