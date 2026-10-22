use std::collections::HashMap;
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
use crate::render::View;

pub struct DataSnapshot {
    pub hn_pages: HnPages,
    pub gh_trending: GhTrending,
    pub reddit_feed: RedditFeed,
    pub last_fetched: DateTime<Utc>,
}

/// Pre-rendered + compressed HTML. `Bytes` fields are cheap (refcount) clones on each request.
pub struct HtmlSnapshot {
    pub html: Bytes,
    pub gzip: Bytes,
    pub zstd: Bytes,
    pub etag: String,
    pub refresh_secs: u64,
    pub is_loading: bool,
    pub version: i64,
}

impl HtmlSnapshot {
    pub fn from_data(data: &DataSnapshot, view: &View) -> Option<Self> {
        let view = view.clone().normalized();
        let html = render::render_page(
            &data.hn_pages,
            &data.gh_trending,
            &data.reddit_feed,
            data.last_fetched,
            &view,
        );
        let version = data.last_fetched.timestamp();
        let time_bucket = Utc::now().timestamp() / config::HTML_REFRESH_SECS as i64;
        let etag = weak_etag(version, &view, time_bucket);
        Self::encode(html, config::HTML_REFRESH_SECS, false, version, etag)
    }

    /// Loading page: short 3s TTL + Refresh header for auto-reload until data arrives.
    pub fn loading() -> Self {
        let html = render::render_loading_page();
        let etag = compute_etag(html.as_bytes());
        Self::encode(html, config::LOADING_PAGE_TTL_SECS, true, 0, etag)
            .expect("startup: compression failed")
    }

    fn encode(
        html: String,
        refresh_secs: u64,
        is_loading: bool,
        version: i64,
        etag: String,
    ) -> Option<Self> {
        let (html, gzip, zstd) = encode_parts(html)?;
        Some(Self {
            html,
            gzip,
            zstd,
            etag,
            refresh_secs,
            is_loading,
            version,
        })
    }
}

/// One independently addressable list, precompressed for both encodings.
pub struct Fragment {
    pub html: Bytes,
    pub gzip: Bytes,
    pub zstd: Bytes,
    pub etag: String,
}

/// Every list the client can request, rendered from one data snapshot. Fragments
/// are immutable for their version; only data updates rebuild them.
pub struct FragmentSnapshot {
    pub version: i64,
    pub fragments: HashMap<String, Fragment>,
}

impl FragmentSnapshot {
    pub fn from_data(data: &DataSnapshot) -> Self {
        let version = data.last_fetched.timestamp();
        let attr_version = version.max(0) as u64;
        let mut fragments = HashMap::new();

        for &(name, _) in config::HN_PAGES {
            let html = render::render_hn_fragment(name, data.hn_pages.get(name), attr_version)
                .into_string();
            insert_fragment(&mut fragments, hn_key(name), html, version);
        }

        for &period in config::GITHUB_PERIODS {
            let mut langs: Vec<String> = config::GITHUB_LANGUAGES
                .iter()
                .map(|&(name, _)| name.to_lowercase())
                .collect();
            langs.push(config::FILTER_ALL.to_string());

            for lang in langs {
                let Some(markup) =
                    render::render_gh_fragment(&data.gh_trending, period, &lang, attr_version)
                else {
                    tracing::warn!(period, lang, "fragment: unknown language");
                    continue;
                };
                insert_fragment(
                    &mut fragments,
                    gh_key(period, &lang),
                    markup.into_string(),
                    version,
                );
            }
        }

        for sub in
            std::iter::once(config::FILTER_ALL).chain(config::REDDIT_SUBREDDITS.iter().copied())
        {
            let html =
                render::render_reddit_fragment(&data.reddit_feed, sub, attr_version).into_string();
            insert_fragment(&mut fragments, reddit_key(sub), html, version);
        }

        Self { version, fragments }
    }

    pub fn get(&self, key: &str) -> Option<&Fragment> {
        self.fragments.get(key)
    }
}

fn insert_fragment(
    fragments: &mut HashMap<String, Fragment>,
    key: String,
    html: String,
    version: i64,
) {
    let etag = fragment_etag(version, &key);
    let Some((html, gzip, zstd)) = encode_parts(html) else {
        tracing::error!(key, "fragment: compression failed");
        return;
    };
    fragments.insert(
        key,
        Fragment {
            html,
            gzip,
            zstd,
            etag,
        },
    );
}

/// A private fragment encoded for exactly one response negotiation. Unlike a
/// shared `Fragment`, this does not compress both gzip and zstd up front.
#[derive(Clone)]
pub struct EncodedFragment {
    pub body: Bytes,
    pub encoding: Option<&'static str>,
    pub etag: String,
}

pub fn encode_custom_fragment(
    html: String,
    version: i64,
    key: &str,
    accept_zstd: bool,
    accept_gzip: bool,
) -> Option<EncodedFragment> {
    let etag = fragment_etag(version, key);
    let (body, encoding) = if accept_zstd {
        (
            Bytes::from(compress_zstd(html.as_bytes()).ok()?),
            Some("zstd"),
        )
    } else if accept_gzip {
        (
            Bytes::from(compress_gzip(html.as_bytes()).ok()?),
            Some("gzip"),
        )
    } else {
        (Bytes::from(html), None)
    };
    Some(EncodedFragment {
        body,
        encoding,
        etag,
    })
}

fn encode_parts(html: String) -> Option<(Bytes, Bytes, Bytes)> {
    let gzip = compress_gzip(html.as_bytes())
        .map_err(|e| tracing::error!(error = %e, "gzip compression failed"))
        .ok()?;
    let zstd = compress_zstd(html.as_bytes())
        .map_err(|e| tracing::error!(error = %e, "zstd compression failed"))
        .ok()?;
    Some((Bytes::from(html), Bytes::from(gzip), Bytes::from(zstd)))
}

pub fn hn_key(page: &str) -> String {
    format!("hn:{page}")
}

pub fn gh_key(period: &str, lang: &str) -> String {
    format!("gh:{period}:{lang}")
}

pub fn reddit_key(sub: &str) -> String {
    format!("reddit:{sub}")
}

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn fnv1a(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// FNV-1a hash — deterministic across restarts (unlike DefaultHasher).
pub(crate) fn compute_etag(data: &[u8]) -> String {
    format!("\"{:x}\"", fnv1a(data))
}

/// Weak validator: the relative-time text changes every minute, so the render
/// bucket is part of the validator. Clients refresh times after load, but a
/// no-JS client must still revalidate to receive the new shell.
fn weak_etag(version: i64, view: &View, time_bucket: i64) -> String {
    let key = format!("{}|{}|{}|{}", view.hn, view.period, view.lang, view.sub);
    format!("W/\"{version}-{time_bucket}-{:x}\"", fnv1a(key.as_bytes()))
}

fn fragment_etag(version: i64, key: &str) -> String {
    format!("\"{version}-{:x}\"", fnv1a(key.as_bytes()))
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
pub type SharedFragments = Arc<ArcSwap<FragmentSnapshot>>;

#[derive(Clone)]
pub struct AppState {
    pub data: SharedData,
    pub html: SharedHtml,
    pub fragments: SharedFragments,
}

pub fn new_shared_data(restored: Option<DataSnapshot>) -> SharedData {
    let snapshot = restored.unwrap_or_else(|| DataSnapshot {
        hn_pages: HnPages::new(),
        gh_trending: GhTrending::new(),
        reddit_feed: RedditFeed::new(),
        last_fetched: DateTime::UNIX_EPOCH,
    });
    Arc::new(ArcSwap::new(Arc::new(snapshot)))
}

/// Renders restored data so the first paint has real panels; falls back to the
/// loading page when nothing was restored or rendering fails.
pub fn new_shared_html(data: &SharedData) -> SharedHtml {
    let snapshot = data.load_full();
    let html = if snapshot.last_fetched.timestamp() > 0 {
        HtmlSnapshot::from_data(&snapshot, &View::default()).unwrap_or_else(|| {
            tracing::warn!("startup: rendering restored data failed, showing loading page");
            HtmlSnapshot::loading()
        })
    } else {
        HtmlSnapshot::loading()
    };
    Arc::new(ArcSwap::new(Arc::new(html)))
}

pub fn new_shared_fragments(data: &SharedData) -> SharedFragments {
    let snapshot = data.load_full();
    Arc::new(ArcSwap::new(Arc::new(FragmentSnapshot::from_data(
        &snapshot,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> DataSnapshot {
        DataSnapshot {
            hn_pages: HnPages::new(),
            gh_trending: GhTrending::new(),
            reddit_feed: RedditFeed::new(),
            last_fetched: Utc::now(),
        }
    }

    #[test]
    fn restored_snapshot_renders_instead_of_loading() {
        let data = new_shared_data(Some(sample_snapshot()));
        let html = new_shared_html(&data);

        assert!(!html.load().is_loading);
        assert!(html.load().html.len() > 1000);
    }

    #[test]
    fn empty_snapshot_shows_loading_page() {
        let data = new_shared_data(None);
        let html = new_shared_html(&data);

        assert!(html.load().is_loading);
    }

    #[test]
    fn fragments_are_stable_for_a_version() {
        let data = sample_snapshot();
        let first = FragmentSnapshot::from_data(&data);
        let second = FragmentSnapshot::from_data(&data);

        assert_eq!(first.version, second.version);
        for (key, fragment) in &first.fragments {
            let other = second.get(key).expect("same key");
            assert_eq!(fragment.etag, other.etag, "{key}");
            assert_eq!(fragment.html, other.html, "{key}");
            assert!(!fragment.gzip.is_empty());
            assert!(!fragment.zstd.is_empty());
        }
    }

    #[test]
    fn shell_etag_is_stable_within_a_render_bucket() {
        let view = View::default();
        assert_eq!(weak_etag(1, &view, 7), weak_etag(1, &view, 7));
        assert!(weak_etag(1, &view, 7).starts_with("W/\""));
    }

    #[test]
    fn shell_etag_changes_with_the_render_bucket() {
        let view = View::default();
        assert_ne!(weak_etag(1, &view, 7), weak_etag(1, &view, 8));
    }

    #[test]
    fn different_views_have_different_etags() {
        let data = new_shared_data(Some(sample_snapshot()));
        let snapshot = data.load_full();
        let default = HtmlSnapshot::from_data(&snapshot, &View::default()).expect("shell");
        let other = HtmlSnapshot::from_data(
            &snapshot,
            &View {
                hn: "newest".into(),
                ..View::default()
            },
        )
        .expect("shell");

        assert_ne!(default.etag, other.etag);
    }
}
