pub mod github;
pub mod hackernews;
pub mod reddit;
pub mod reddit_auth;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest_middleware::Error),
    #[error("background task panicked: {0}")]
    TaskPanic(#[from] tokio::task::JoinError),
    /// A 200 response that yielded nothing. Distinct from a genuinely empty
    /// listing: the caller keeps its previous data rather than blanking a panel.
    #[error("page loaded but no rows matched — markup changed or IP blocked")]
    EmptyPage,
    /// The Reddit token request failed this cycle; its error is logged once.
    #[error("reddit token unavailable")]
    TokenUnavailable,
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

/// The error's message followed by each distinct cause, joined by ": ".
/// reqwest's own message stops at "error sending request for url (…)"; the
/// cause (timeout, connection reset, DNS, TLS) is only in the source chain.
pub(crate) fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        // Wrappers often repeat their source's text; append only what is new.
        if !message.contains(&text) {
            message.push_str(": ");
            message.push_str(&text);
        }
        source = cause.source();
    }
    message
}

#[cfg(test)]
pub(crate) mod tests {
    pub fn test_clients() -> crate::client::Clients {
        crate::client::build_clients()
    }

    #[derive(Debug, thiserror::Error)]
    #[error("{message}")]
    struct Wrapped {
        message: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    }

    #[test]
    fn error_chain_appends_distinct_causes() {
        let io = std::io::Error::new(std::io::ErrorKind::TimedOut, "operation timed out");
        let inner = Wrapped {
            message: "error sending request".into(),
            source: Box::new(io),
        };
        // A wrapper that repeats its source's text, like FetchError::Http does.
        let outer = Wrapped {
            message: "HTTP request failed: error sending request".into(),
            source: Box::new(inner),
        };
        assert_eq!(
            super::error_chain(&outer),
            "HTTP request failed: error sending request: operation timed out"
        );
    }

    #[test]
    fn error_chain_without_source_is_the_message() {
        let io = std::io::Error::other("boom");
        assert_eq!(super::error_chain(&io), "boom");
    }
}
