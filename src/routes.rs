use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{Path, RawQuery, Request, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, COOKIE, ETAG, HeaderName, HeaderValue,
    IF_NONE_MATCH, REFERRER_POLICY, REFRESH, VARY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use chrono::Utc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tower::ServiceBuilder;
use tower::timeout::TimeoutLayer;
use tower_http::compression::CompressionLayer;

use crate::cache::{self, AppState, Fragment, HtmlSnapshot};
use crate::config;
use crate::render::{self, View};

const X_DATA_VERSION: &str = "x-data-version";
const X_CUSTOM_FRAGMENT: &str = "x-tty1-custom";
const VARY_PAGE: &str = "Accept-Encoding, Cookie";

static RENDER_SLOTS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(config::MAX_CONCURRENT_RENDERS)));

fn try_render_slot() -> Option<OwnedSemaphorePermit> {
    RENDER_SLOTS.clone().try_acquire_owned().ok()
}

struct CacheState<K, V> {
    values: HashMap<K, Arc<V>>,
    order: VecDeque<K>,
}

struct BoundedCache<K, V> {
    capacity: usize,
    state: Mutex<CacheState<K, V>>,
}

impl<K, V> BoundedCache<K, V>
where
    K: Eq + Hash + Clone,
{
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: Mutex::new(CacheState {
                values: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    fn get(&self, key: &K) -> Option<Arc<V>> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values
            .get(key)
            .cloned()
    }

    fn insert(&self, key: K, value: Arc<V>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(slot) = state.values.get_mut(&key) {
            *slot = value;
            return;
        }
        if state.values.len() >= self.capacity
            && let Some(oldest) = state.order.pop_front()
        {
            state.values.remove(&oldest);
        }
        state.order.push_back(key.clone());
        state.values.insert(key, value);
    }
}

struct SettingsPage {
    html: Bytes,
    etag: String,
}

static SETTINGS_PAGE: LazyLock<SettingsPage> = LazyLock::new(|| {
    let html = Bytes::from(render::render_settings_page());
    let etag = cache::compute_etag(&html);
    SettingsPage { html, etag }
});

static CUSTOM_PAGES: LazyLock<BoundedCache<String, HtmlSnapshot>> =
    LazyLock::new(|| BoundedCache::new(config::CUSTOM_PAGE_CACHE_ENTRIES));
static CUSTOM_FRAGMENTS: LazyLock<BoundedCache<String, cache::EncodedFragment>> =
    LazyLock::new(|| BoundedCache::new(config::CUSTOM_FRAGMENT_CACHE_ENTRIES));

// ── Router ──────────────────────────────────────────────────────────

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/settings", get(settings))
        .route("/api/health", get(api_health))
        .route("/api/version", get(api_version))
        .route("/f/hn/{page}", get(fragment_hn))
        .route("/f/gh/{period}/{lang}", get(fragment_gh))
        .route("/f/reddit/{sub}", get(fragment_reddit))
        .route("/favicon.svg", get(favicon))
        .route("/icon-192.png", get(app_icon_192))
        .route("/icon-512.png", get(app_icon_512))
        .route("/apple-touch-icon.png", get(apple_touch_icon))
        .route("/manifest.json", get(manifest))
        .route("/sw.js", get(sw))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|_: tower::BoxError| async {
                    StatusCode::REQUEST_TIMEOUT
                }))
                .layer(TimeoutLayer::new(Duration::from_secs(
                    config::HANDLER_TIMEOUT_SECS,
                )))
                .layer(CompressionLayer::new()),
        )
}

// ── Security middleware ─────────────────────────────────────────────

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    h.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
    );
    h.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    h.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_str(&crate::csp::CSP_HEADER).expect("valid CSP header"),
    );
    response
}

// ── Page ────────────────────────────────────────────────────────────

async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    let cookies = parse_cookies(&headers);
    let query = parse_query(raw.as_deref());
    let (view, personalized) = select_view(&query, &cookies);

    let loading = state.html.load().is_loading;
    let snapshot = if loading || view == View::default() {
        state.html.load_full()
    } else {
        let data = state.data.load_full();
        let version = data.last_fetched.timestamp();
        let time_bucket = Utc::now().timestamp() / config::HTML_REFRESH_SECS as i64;
        let cache_key = format!(
            "{version}:{time_bucket}:{}|{}|{}|{}",
            view.hn, view.period, view.lang, view.sub
        );
        if let Some(snapshot) = CUSTOM_PAGES.get(&cache_key) {
            snapshot
        } else {
            let Some(permit) = try_render_slot() else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            let request_view = view.clone();
            let started = Instant::now();
            // Maud rendering and compression are CPU-bound; keep them off the
            // Tokio worker that fragment requests need.
            match tokio::task::spawn_blocking(move || {
                let _permit = permit;
                HtmlSnapshot::from_data(&data, &request_view)
            })
            .await
            {
                Ok(Some(snapshot)) => {
                    tracing::debug!(
                        elapsed_ms = started.elapsed().as_millis(),
                        "custom shell rendered"
                    );
                    let snapshot = Arc::new(snapshot);
                    CUSTOM_PAGES.insert(cache_key, snapshot.clone());
                    snapshot
                }
                _ => state.html.load_full(),
            }
        }
    };

    page_response(&snapshot, &headers, personalized)
}

fn page_response(snapshot: &HtmlSnapshot, headers: &HeaderMap, personalized: bool) -> Response {
    let max_age = snapshot.refresh_secs.to_string();
    // The loading page must never be cached: the service worker would serve it
    // as the first paint long after real data arrived.
    let cache_control = if snapshot.is_loading {
        "no-store".to_string()
    } else if personalized {
        format!("private, max-age={max_age}")
    } else {
        format!("public, max-age={max_age}")
    };

    if let Some(r) = not_modified(headers, &snapshot.etag, &cache_control, VARY_PAGE) {
        return r;
    }

    let accept = headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let selected = preferred_encoding(accept);
    let (body, encoding) = match selected {
        Some("zstd") => (snapshot.zstd.clone(), Some("zstd")),
        Some("gzip") => (snapshot.gzip.clone(), Some("gzip")),
        _ => (snapshot.html.clone(), None),
    };

    let mut builder = Response::builder()
        .header(VARY, VARY_PAGE)
        .header(ETAG, &*snapshot.etag)
        .header(CACHE_CONTROL, &*cache_control)
        .header(X_DATA_VERSION, snapshot.version.to_string())
        .header(CONTENT_TYPE, "text/html; charset=utf-8");

    if let Some(enc) = encoding {
        builder = builder.header(CONTENT_ENCODING, enc);
    }
    // Auto-refresh loading page only — data pages don't refresh to preserve scroll/tab state.
    if snapshot.is_loading {
        builder = builder.header(REFRESH, &*max_age);
    }
    builder.body(Body::from(body)).expect("valid response")
}

async fn settings(headers: HeaderMap) -> Response {
    if let Some(response) =
        not_modified(&headers, &SETTINGS_PAGE.etag, "no-cache", "Accept-Encoding")
    {
        return response;
    }
    Response::builder()
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(VARY, "Accept-Encoding")
        .header(ETAG, &SETTINGS_PAGE.etag)
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from(SETTINGS_PAGE.html.clone()))
        .expect("valid response")
}

// ── Fragment API ────────────────────────────────────────────────────

async fn fragment_hn(
    State(state): State<AppState>,
    Path(page): Path<String>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    fragment_response(&state, &cache::hn_key(&page), &headers, &raw)
}

async fn fragment_gh(
    State(state): State<AppState>,
    Path((period, lang)): Path<(String, String)>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    let query = parse_query(raw.as_deref());
    if lang == "mine" {
        return custom_gh_fragment(&state, &period, query.get("langs"), &headers).await;
    }
    fragment_response(&state, &cache::gh_key(&period, &lang), &headers, &raw)
}

async fn fragment_reddit(
    State(state): State<AppState>,
    Path(sub): Path<String>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    let query = parse_query(raw.as_deref());
    if sub == config::FILTER_ALL && query.contains_key("subs") {
        return custom_reddit_fragment(&state, query.get("subs"), &headers).await;
    }
    fragment_response(&state, &cache::reddit_key(&sub), &headers, &raw)
}

fn custom_subs(raw: Option<&String>) -> Vec<String> {
    let mut subs: Vec<String> = raw
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(|sub| sub.trim().to_lowercase())
        .filter(|sub| {
            config::REDDIT_SUBREDDITS
                .iter()
                .any(|configured| configured.eq_ignore_ascii_case(sub))
        })
        .collect();
    subs.sort_unstable();
    subs.dedup();
    subs
}

fn custom_langs(raw: Option<&String>) -> Vec<String> {
    let mut langs: Vec<String> = raw
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(|lang| lang.trim().to_lowercase())
        .filter(|lang| lang != config::FILTER_ALL && render::valid_lang(lang))
        .collect();
    langs.sort_unstable();
    langs.dedup();
    langs
}

async fn custom_reddit_fragment(
    state: &AppState,
    raw_subs: Option<&String>,
    headers: &HeaderMap,
) -> Response {
    let subs = custom_subs(raw_subs);
    if subs.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let data = state.data.load_full();
    let version = data.last_fetched.timestamp();
    let key = format!("reddit:all:{}", subs.join(","));
    let accept = headers
        .get("accept-encoding")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let selected = preferred_encoding(accept);
    let accept_zstd = selected == Some("zstd");
    let accept_gzip = selected == Some("gzip");
    let cache_key = format!("{version}:{accept_zstd}:{accept_gzip}:{key}");
    if let Some(fragment) = CUSTOM_FRAGMENTS.get(&cache_key) {
        return custom_fragment_response((*fragment).clone(), version);
    }
    let Some(permit) = try_render_slot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let started = Instant::now();
        let mut posts = Vec::new();
        for sub in &subs {
            if let Some(items) = data.reddit_feed.get(sub) {
                posts.extend(items.iter().cloned());
            }
        }
        posts.sort_by_key(|post| std::cmp::Reverse(post.score));
        posts.truncate(config::CUSTOM_LIST_LIMIT);
        let html =
            render::render_reddit_custom_fragment(&posts, version.max(0) as u64).into_string();
        let fragment =
            cache::encode_custom_fragment(html, version, &key, accept_zstd, accept_gzip)?;
        CUSTOM_FRAGMENTS.insert(cache_key, Arc::new(fragment.clone()));
        tracing::debug!(
            provider = "reddit",
            posts = posts.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "custom fragment rendered"
        );
        Some(fragment)
    })
    .await;

    match result {
        Ok(Some(fragment)) => custom_fragment_response(fragment, version),
        Ok(None) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "custom reddit fragment task failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn custom_gh_fragment(
    state: &AppState,
    period: &str,
    raw_langs: Option<&String>,
    headers: &HeaderMap,
) -> Response {
    if !render::valid_period(period) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let langs = custom_langs(raw_langs);
    if langs.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let data = state.data.load_full();
    let version = data.last_fetched.timestamp();
    let key = format!("gh:{period}:mine:{}", langs.join(","));
    let period = period.to_string();
    let accept = headers
        .get("accept-encoding")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let selected = preferred_encoding(accept);
    let accept_zstd = selected == Some("zstd");
    let accept_gzip = selected == Some("gzip");
    let cache_key = format!("{version}:{accept_zstd}:{accept_gzip}:{key}");
    if let Some(fragment) = CUSTOM_FRAGMENTS.get(&cache_key) {
        return custom_fragment_response((*fragment).clone(), version);
    }
    let Some(permit) = try_render_slot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let started = Instant::now();
        let mut repos = Vec::new();
        for lang in &langs {
            let Some(display) = render::lang_display(lang) else {
                continue;
            };
            if let Some(items) = data.gh_trending.get(&(period.clone(), display.to_string())) {
                repos.extend(items.iter().cloned());
            }
        }
        repos.sort_by_key(|repo| std::cmp::Reverse(repo.stars));
        repos.truncate(config::CUSTOM_LIST_LIMIT);
        let html = render::render_gh_custom_fragment(&repos, version.max(0) as u64).into_string();
        let fragment =
            cache::encode_custom_fragment(html, version, &key, accept_zstd, accept_gzip)?;
        CUSTOM_FRAGMENTS.insert(cache_key, Arc::new(fragment.clone()));
        tracing::debug!(
            provider = "github",
            repos = repos.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "custom fragment rendered"
        );
        Some(fragment)
    })
    .await;

    match result {
        Ok(Some(fragment)) => custom_fragment_response(fragment, version),
        Ok(None) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "custom github fragment task failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn custom_fragment_response(fragment: cache::EncodedFragment, version: i64) -> Response {
    let mut builder = Response::builder()
        .header(VARY, "Accept-Encoding")
        .header(ETAG, fragment.etag)
        .header(CACHE_CONTROL, "private, no-store")
        .header(X_DATA_VERSION, version.to_string())
        .header(X_CUSTOM_FRAGMENT, "1")
        .header(CONTENT_TYPE, "text/html; charset=utf-8");
    if let Some(encoding) = fragment.encoding {
        builder = builder.header(CONTENT_ENCODING, encoding);
    }
    builder
        .body(Body::from(fragment.body))
        .expect("valid custom fragment response")
}

fn fragment_response(
    state: &AppState,
    key: &str,
    headers: &HeaderMap,
    raw_query: &Option<String>,
) -> Response {
    let snapshot = state.fragments.load();
    let Some(fragment) = snapshot.get(key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let query = parse_query(raw_query.as_deref());
    let version_matches = match query.get("v") {
        Some(requested) => *requested == snapshot.version.to_string(),
        None => true,
    };
    fragment_body(fragment, snapshot.version, headers, version_matches)
}

fn fragment_body(
    fragment: &Fragment,
    version: i64,
    headers: &HeaderMap,
    version_matches: bool,
) -> Response {
    // A client can briefly hold a newer shell than the fragment store. Do not
    // let that response be cached under the requested version.
    let cache_control = if version_matches {
        "public, max-age=60"
    } else {
        "no-store"
    };
    if version_matches
        && let Some(r) = not_modified(headers, &fragment.etag, cache_control, "Accept-Encoding")
    {
        return r;
    }

    let accept = headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let selected = preferred_encoding(accept);
    let (body, encoding) = match selected {
        Some("zstd") => (fragment.zstd.clone(), Some("zstd")),
        Some("gzip") => (fragment.gzip.clone(), Some("gzip")),
        _ => (fragment.html.clone(), None),
    };

    let mut builder = Response::builder()
        .header(VARY, "Accept-Encoding")
        .header(ETAG, &*fragment.etag)
        .header(CACHE_CONTROL, cache_control)
        .header(X_DATA_VERSION, version.to_string())
        .header(CONTENT_TYPE, "text/html; charset=utf-8");

    if let Some(enc) = encoding {
        builder = builder.header(CONTENT_ENCODING, enc);
    }
    builder.body(Body::from(body)).expect("valid response")
}

// ── API ─────────────────────────────────────────────────────────────

async fn api_health(State(state): State<AppState>) -> Response {
    let (status, body) = if state.data.load().last_fetched.timestamp() > 0 {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "loading")
    };
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .expect("valid response")
}

/// Cheap poll target for the dashboard's auto-update. About 40 bytes, so a tab
/// can check every minute without refetching the page.
async fn api_version(State(state): State<AppState>) -> Response {
    let html = state.html.load();
    let body = serde_json::json!({
        "last_fetched": html.version,
        "etag": html.etag,
    })
    .to_string();

    Response::builder()
        .header(CONTENT_TYPE, "application/json")
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .expect("valid response")
}

// ── Static assets ───────────────────────────────────────────────────

async fn favicon() -> Response {
    static_response(
        include_str!("static/icons/favicon.svg"),
        "image/svg+xml",
        "public, max-age=31536000, immutable",
    )
}

async fn app_icon_192() -> Response {
    static_bytes_response(
        include_bytes!("static/icons/icon-192.png"),
        "image/png",
        "public, max-age=31536000, immutable",
    )
}

async fn app_icon_512() -> Response {
    static_bytes_response(
        include_bytes!("static/icons/icon-512.png"),
        "image/png",
        "public, max-age=31536000, immutable",
    )
}

async fn apple_touch_icon() -> Response {
    static_bytes_response(
        include_bytes!("static/icons/apple-touch-icon.png"),
        "image/png",
        "public, max-age=31536000, immutable",
    )
}

async fn manifest() -> Response {
    static_response(
        include_str!("static/manifest.json"),
        "application/manifest+json",
        "public, max-age=3600",
    )
}

async fn sw() -> Response {
    static_response(
        include_str!("static/sw.js"),
        "application/javascript",
        "no-cache",
    )
}

fn static_response(
    body: &'static str,
    content_type: &'static str,
    cache_control: &'static str,
) -> Response {
    Response::builder()
        .header(CONTENT_TYPE, content_type)
        .header(CACHE_CONTROL, cache_control)
        .body(Body::from(body))
        .expect("valid response")
}

fn static_bytes_response(
    body: &'static [u8],
    content_type: &'static str,
    cache_control: &'static str,
) -> Response {
    Response::builder()
        .header(CONTENT_TYPE, content_type)
        .header(CACHE_CONTROL, cache_control)
        .body(Body::from(body))
        .expect("valid response")
}

// ── View selection ──────────────────────────────────────────────────

/// Query params win over cookies; invalid values fall through to the next
/// source. `personalized` is true when a cookie supplied a valid value.
fn select_view(query: &HashMap<String, String>, cookies: &HashMap<String, String>) -> (View, bool) {
    let default = View::default();
    let (hn, hn_cookie) = pick(
        query.get("hn").map(String::as_str),
        cookies.get("tty1_hn").map(String::as_str),
        render::valid_hn,
    );
    let (period, period_cookie) = pick(
        query.get("period").map(String::as_str),
        cookies.get("tty1_period").map(String::as_str),
        render::valid_period,
    );
    let (lang, lang_cookie) = pick(
        query.get("lang").map(String::as_str),
        cookies.get("tty1_lang").map(String::as_str),
        render::valid_lang,
    );
    let (sub, sub_cookie) = pick(
        query.get("sub").map(String::as_str),
        cookies.get("tty1_sub").map(String::as_str),
        render::valid_sub,
    );

    let view = View {
        hn: hn.unwrap_or(&default.hn).to_string(),
        period: period.unwrap_or(&default.period).to_string(),
        lang: lang.unwrap_or(&default.lang).to_string(),
        sub: sub.unwrap_or(&default.sub).to_string(),
    }
    .normalized();

    (
        view,
        hn_cookie || period_cookie || lang_cookie || sub_cookie,
    )
}

fn pick<'a>(
    query: Option<&'a str>,
    cookie: Option<&'a str>,
    valid: fn(&str) -> bool,
) -> (Option<&'a str>, bool) {
    if let Some(value) = query.filter(|value| valid(value)) {
        return (Some(value), false);
    }
    if let Some(value) = cookie.filter(|value| valid(value)) {
        return (Some(value), true);
    }
    (None, false)
}

/// First value wins for repeated keys, so a hand-edited or doubled query
/// parameter cannot turn the dashboard into a 400.
fn parse_query(raw: Option<&str>) -> HashMap<String, String> {
    let mut query = HashMap::new();
    for pair in raw.unwrap_or("").split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = decode_component(name, true);
        if name.is_empty() {
            continue;
        }
        query
            .entry(name)
            .or_insert_with(|| decode_component(raw_value, true));
    }
    query
}

fn parse_cookies(headers: &HeaderMap) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for header in headers.get_all(COOKIE) {
        let Ok(value) = header.to_str() else {
            continue;
        };
        for pair in value.split(';') {
            let Some((name, raw)) = pair.trim().split_once('=') else {
                continue;
            };
            cookies.insert(name.trim().to_string(), percent_decode(raw.trim()));
        }
    }
    cookies
}

fn percent_decode(input: &str) -> String {
    decode_component(input, false)
}

fn decode_component(input: &str, plus_as_space: bool) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if plus_as_space && bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// ── Response helpers ────────────────────────────────────────────────

/// Returns a 304 Not Modified response if If-None-Match matches the ETag.
/// Handles multi-value headers and the wildcard `*`.
fn not_modified(
    headers: &HeaderMap,
    etag: &str,
    cache_control: &str,
    vary: &str,
) -> Option<Response> {
    let matched = headers
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|candidate| {
            let candidate = candidate.trim();
            if candidate == "*" {
                return true;
            }
            candidate.strip_prefix("W/").unwrap_or(candidate)
                == etag.strip_prefix("W/").unwrap_or(etag)
        });
    if !matched {
        return None;
    }
    Some(
        Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(VARY, vary)
            .header(ETAG, etag)
            .header(CACHE_CONTROL, cache_control)
            .body(Body::empty())
            .expect("valid response"),
    )
}

fn encoding_quality(accept: &str, encoding: &str) -> f64 {
    let mut wildcard = None;
    for part in accept.split(',') {
        let mut pieces = part.trim().split(';');
        let name = pieces.next().unwrap_or("").trim();
        let quality = pieces
            .filter_map(|parameter| {
                parameter
                    .trim()
                    .strip_prefix("q=")
                    .and_then(|value| value.trim().parse::<f64>().ok())
            })
            .next()
            .unwrap_or(1.0);
        if name.eq_ignore_ascii_case(encoding) {
            return quality;
        }
        if name == "*" {
            wildcard = Some(quality);
        }
    }
    wildcard.unwrap_or(0.0)
}

fn preferred_encoding(accept: &str) -> Option<&'static str> {
    let zstd = encoding_quality(accept, "zstd");
    let gzip = encoding_quality(accept, "gzip");
    if zstd > 0.0 && zstd >= gzip {
        Some("zstd")
    } else if gzip > 0.0 {
        Some("gzip")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_modified_reads_every_header_line() {
        let mut headers = HeaderMap::new();
        headers.append(IF_NONE_MATCH, HeaderValue::from_static("\"other\""));
        headers.append(IF_NONE_MATCH, HeaderValue::from_static("\"etag\""));

        let response = not_modified(
            &headers,
            "\"etag\"",
            "public, max-age=60",
            "Accept-Encoding",
        )
        .expect("matching validator");

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            response
                .headers()
                .get(VARY)
                .and_then(|value| value.to_str().ok()),
            Some("Accept-Encoding")
        );
    }

    #[test]
    fn preferred_encoding_respects_quality() {
        assert_eq!(preferred_encoding("gzip;q=1, zstd;q=0.5"), Some("gzip"));
        assert_eq!(preferred_encoding("gzip;q=0, *;q=1"), Some("zstd"));
        assert_eq!(preferred_encoding("br"), None);
    }

    #[test]
    fn not_modified_uses_weak_comparison() {
        let mut headers = HeaderMap::new();
        headers.insert(IF_NONE_MATCH, HeaderValue::from_static("\"opaque-tag\""));

        let response = not_modified(
            &headers,
            "W/\"opaque-tag\"",
            "public, max-age=60",
            "Accept-Encoding",
        );

        assert_eq!(
            response.map(|response| response.status()),
            Some(StatusCode::NOT_MODIFIED)
        );
    }

    #[test]
    fn cookie_view_wins_when_query_is_absent() {
        let query = HashMap::new();
        let cookies = HashMap::from([
            ("tty1_hn".to_string(), "newest".to_string()),
            ("tty1_lang".to_string(), "c++".to_string()),
            ("tty1_sub".to_string(), "NixOS".to_string()),
        ]);

        let (view, personalized) = select_view(&query, &cookies);

        assert!(personalized);
        assert_eq!(view.hn, "newest");
        assert_eq!(view.lang, "c++");
        assert_eq!(view.sub, "NixOS");
    }

    #[test]
    fn query_overrides_cookie_and_clears_personalized() {
        let query = HashMap::from([("hn".to_string(), "show".to_string())]);
        let cookies = HashMap::from([("tty1_hn".to_string(), "newest".to_string())]);

        let (view, personalized) = select_view(&query, &cookies);

        assert!(!personalized);
        assert_eq!(view.hn, "show");
    }

    #[test]
    fn custom_subs_filter_sort_and_dedupe() {
        let raw = "Docker,rust,docker,bogus".to_string();
        assert_eq!(custom_subs(Some(&raw)), vec!["docker", "rust"]);
    }

    #[test]
    fn custom_subs_accept_configured_mixed_case_names() {
        let raw = "ClaudeAI,NixOS,OpenAI,docker".to_string();
        assert_eq!(
            custom_subs(Some(&raw)),
            vec!["claudeai", "docker", "nixos", "openai"]
        );
    }

    #[test]
    fn custom_langs_normalize_and_filter() {
        let raw = "Rust,go,rust,bogus,all".to_string();
        assert_eq!(custom_langs(Some(&raw)), vec!["go", "rust"]);
    }

    #[test]
    fn invalid_cookie_values_fall_back() {
        let query = HashMap::new();
        let cookies = HashMap::from([
            ("tty1_hn".to_string(), "bogus".to_string()),
            ("tty1_lang".to_string(), "bogus".to_string()),
            ("tty1_sub".to_string(), "bogus".to_string()),
        ]);

        let (view, personalized) = select_view(&query, &cookies);

        assert!(!personalized);
        assert_eq!(view, View::default());
    }

    #[test]
    fn duplicate_query_params_keep_the_first_value() {
        let query = parse_query(Some("hn=top&hn=show&lang=c%2B%2B"));

        assert_eq!(query.get("hn").map(String::as_str), Some("top"));
        assert_eq!(query.get("lang").map(String::as_str), Some("c++"));
    }

    #[test]
    fn query_decoding_handles_plus_and_percent() {
        let query = parse_query(Some("sub=hello+world&lang=c%2B%2B"));

        assert_eq!(query.get("sub").map(String::as_str), Some("hello world"));
        assert_eq!(query.get("lang").map(String::as_str), Some("c++"));
    }
}
