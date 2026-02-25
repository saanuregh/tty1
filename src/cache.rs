use std::io::Write;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use arc_swap::ArcSwap;
use bytes::Bytes;
use flate2::Compression;
use flate2::write::GzEncoder;

use crate::config;
use crate::providers::github::GhTrending;
use crate::providers::hackernews::HnPages;
use crate::providers::reddit::RedditFeed;
use crate::render;

#[derive(serde::Serialize)]
pub struct DataSnapshot {
    pub hn_pages: HnPages,
    pub gh_trending: GhTrending,
    pub reddit_feed: RedditFeed,
    pub last_fetched: DateTime<Utc>,
    #[serde(skip)]
    pub etag: String,
}

/// Pre-rendered + compressed HTML. `Bytes` fields are cheap (refcount) clones on each request.
pub struct HtmlSnapshot {
    pub html: Bytes,
    pub gzip: Bytes,
    pub zstd: Bytes,
    pub etag: String,
    pub refresh_secs: u64,
    pub is_loading: bool,
}

impl HtmlSnapshot {
    pub fn from_data(data: &DataSnapshot) -> Option<Self> {
        let html = render::render_page(
            &data.hn_pages,
            &data.gh_trending,
            &data.reddit_feed,
            data.last_fetched,
        );
        Self::compress(html, config::HTML_REFRESH_SECS, false)
    }

    /// Loading page: short 3s TTL + Refresh header for auto-reload until data arrives.
    pub fn loading() -> Self {
        let html = render::render_loading_page();
        Self::compress(html, config::LOADING_PAGE_TTL_SECS, true)
            .expect("startup: compression failed")
    }

    fn compress(html: String, refresh_secs: u64, is_loading: bool) -> Option<Self> {
        let etag = compute_etag(html.as_bytes());
        let gzip = compress_gzip(html.as_bytes())
            .map_err(|e| tracing::error!(error = %e, "gzip compression failed"))
            .ok()?;
        let zstd = compress_zstd(html.as_bytes())
            .map_err(|e| tracing::error!(error = %e, "zstd compression failed"))
            .ok()?;
        Some(Self {
            html: Bytes::from(html),
            gzip: Bytes::from(gzip),
            zstd: Bytes::from(zstd),
            etag,
            refresh_secs,
            is_loading,
        })
    }
}

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a hash — deterministic across restarts (unlike DefaultHasher).
pub fn compute_etag(data: &[u8]) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("\"{hash:x}\"")
}

fn compress_gzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(config::GZIP_LEVEL));
    encoder.write_all(data)?;
    encoder.finish()
}

fn compress_zstd(data: &[u8]) -> std::io::Result<Vec<u8>> {
    zstd::encode_all(data, config::ZSTD_LEVEL)
}

/// ArcSwap gives lock-free reads under concurrent requests (vs RwLock which blocks readers during writes).
pub type SharedData = Arc<ArcSwap<DataSnapshot>>;
pub type SharedHtml = Arc<ArcSwap<HtmlSnapshot>>;

#[derive(Clone)]
pub struct AppState {
    pub data: SharedData,
    pub html: SharedHtml,
}

pub fn new_shared_data() -> SharedData {
    Arc::new(ArcSwap::new(Arc::new(DataSnapshot {
        hn_pages: HnPages::new(),
        gh_trending: GhTrending::new(),
        reddit_feed: RedditFeed::new(),
        last_fetched: DateTime::UNIX_EPOCH,
        etag: compute_etag(b"empty"),
    })))
}

pub fn new_shared_html() -> SharedHtml {
    Arc::new(ArcSwap::new(Arc::new(HtmlSnapshot::loading())))
}
