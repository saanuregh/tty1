use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{Request, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG, HeaderName, HeaderValue, IF_NONE_MATCH,
    REFERRER_POLICY, REFRESH, VARY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use tower::ServiceBuilder;
use tower::timeout::TimeoutLayer;
use tower_http::compression::CompressionLayer;

use crate::cache::AppState;
use crate::config;

// ── Router ──────────────────────────────────────────────────────────

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/settings", get(settings))
        .route("/api/data", get(api_data))
        .route("/api/health", get(api_health))
        .route("/favicon.svg", get(favicon))
        .route("/icon.svg", get(app_icon))
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
    response
}

// ── Page ────────────────────────────────────────────────────────────

async fn index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let snapshot = state.html.load();
    let max_age = snapshot.refresh_secs.to_string();
    let cache_control = format!("public, max-age={max_age}");

    if let Some(r) = not_modified(&headers, &snapshot.etag, &cache_control) {
        return r;
    }

    let accept = headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Prefer zstd > gzip > identity. Bytes::clone() is a cheap refcount bump.
    let (body, encoding) = if accepts_encoding(accept, "zstd") {
        (snapshot.zstd.clone(), Some("zstd"))
    } else if accepts_encoding(accept, "gzip") {
        (snapshot.gzip.clone(), Some("gzip"))
    } else {
        (snapshot.html.clone(), None)
    };

    let mut builder = Response::builder()
        .header(VARY, "Accept-Encoding")
        .header(ETAG, &*snapshot.etag)
        .header(CACHE_CONTROL, &*cache_control)
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

async fn settings() -> Response {
    let html = crate::render::render_settings_page();
    Response::builder()
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from(html))
        .expect("valid response")
}

// ── API ─────────────────────────────────────────────────────────────

async fn api_data(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let snapshot = state.data.load_full();

    if let Some(r) = not_modified(&headers, &snapshot.etag, "public, max-age=60") {
        return r;
    }

    let body = match serde_json::to_vec(&*snapshot) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize data snapshot");
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .expect("valid response");
        }
    };

    Response::builder()
        .header(CONTENT_TYPE, "application/json")
        .header(CACHE_CONTROL, "public, max-age=60")
        .header(ETAG, &*snapshot.etag)
        .body(Body::from(body))
        .expect("valid response")
}

async fn api_health(State(state): State<AppState>) -> (StatusCode, &'static str) {
    if state.data.load().last_fetched.timestamp() > 0 {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "loading")
    }
}

// ── Static assets ───────────────────────────────────────────────────

async fn favicon() -> Response {
    static_response(
        include_str!("static/icons/favicon.svg"),
        "image/svg+xml",
        "public, max-age=31536000, immutable",
    )
}

async fn app_icon() -> Response {
    static_response(
        include_str!("static/icons/app-icon.svg"),
        "image/svg+xml",
        "public, max-age=31536000, immutable",
    )
}

async fn manifest() -> Response {
    static_response(
        include_str!("static/manifest.json"),
        "application/manifest+json",
        "public, max-age=31536000, immutable",
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

// ── Response helpers ────────────────────────────────────────────────

/// Returns a 304 Not Modified response if If-None-Match matches the ETag.
/// Handles multi-value headers and the wildcard `*`.
fn not_modified(headers: &HeaderMap, etag: &str, cache_control: &str) -> Option<Response> {
    let inm = headers.get(IF_NONE_MATCH)?.to_str().ok()?;
    if inm != "*" && !inm.split(',').any(|t| t.trim() == etag) {
        return None;
    }
    Some(
        Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(VARY, "Accept-Encoding")
            .header(ETAG, etag)
            .header(CACHE_CONTROL, cache_control)
            .body(Body::empty())
            .expect("valid response"),
    )
}

fn accepts_encoding(accept: &str, encoding: &str) -> bool {
    accept.split(',').any(|part| {
        let (name, params) = part.trim().split_once(';').unwrap_or((part.trim(), ""));
        name.trim() == encoding
            && !params
                .split(';')
                .filter_map(|p| p.trim().strip_prefix("q="))
                .any(|q| q.trim().parse::<f64>().unwrap_or(1.0) == 0.0)
    })
}
