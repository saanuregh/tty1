pub mod github;
pub mod hackernews;
pub mod reddit;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest_middleware::Error),
    #[error("background task panicked: {0}")]
    TaskPanic(#[from] tokio::task::JoinError),
}

/// Wrap raw reqwest errors through the middleware error type so `?` works on
/// `.json()`, `.text()`, and `.error_for_status()` calls.
impl From<reqwest::Error> for FetchError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(reqwest_middleware::Error::Reqwest(e))
    }
}

/// Sleeps `base_ms` jittered by ±`jitter_ms` (uniform). Used to space scraper
/// requests: a varying gap looks less robotic than a fixed interval. `jitter_ms`
/// is clamped to `base_ms` so the delay can never go negative.
pub(crate) async fn jittered_delay(base_ms: u64, jitter_ms: u64) {
    let jitter_ms = jitter_ms.min(base_ms);
    let millis = base_ms - jitter_ms + fastrand::u64(0..=2 * jitter_ms);
    tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
}

/// Collected, trimmed text content of an element. Shared by the HTML scrapers.
pub(crate) fn text(el: scraper::ElementRef) -> String {
    el.text().collect::<String>().trim().to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    pub fn test_client() -> crate::client::Client {
        crate::client::build_client()
    }
}
