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

#[cfg(test)]
pub(crate) mod tests {
    pub fn test_client() -> crate::client::Client {
        crate::client::build_client()
    }
}
