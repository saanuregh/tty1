//! Anonymous OAuth as the official Reddit Android app — the flow Redlib uses.
//! Needs no account: the token endpoint hands any "device" a 24 h bearer token
//! for `oauth.reddit.com`.

use std::time::{Duration, Instant};

use chrono::Utc;
use reqwest::header::{CONTENT_TYPE, USER_AGENT};
use reqwest_middleware::RequestBuilder;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::{FetchError, error_chain};
use crate::client::Client;
use crate::config;

/// One spoofed Android install. Kept for a token's whole lifetime: a real
/// install does not change identity between requests.
#[derive(Clone)]
pub struct Device {
    user_agent: String,
    vendor_id: String,
}

impl Device {
    pub fn random() -> Self {
        let mut rng = fastrand::Rng::new();
        let versions = config::REDDIT_ANDROID_APP_VERSIONS;
        let app_version = versions[rng.usize(..versions.len())];
        let android_version = rng.u8(9..=14);
        Self {
            user_agent: format!("Reddit/{app_version}/Android {android_version}"),
            vendor_id: random_uuid(&mut rng),
        }
    }
}

/// UUID v4 text: 128 random bits with the version nibble set to 4 and the
/// variant bits to `10`.
fn random_uuid(rng: &mut fastrand::Rng) -> String {
    let bits = rng.u128(..);
    let bits = (bits & !(0xF << 76)) | (0x4 << 76);
    let bits = (bits & !(0x3 << 62)) | (0x2 << 62);
    let hex = format!("{bits:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

struct Token {
    access_token: String,
    expires_at: Instant,
    renew_at: Instant,
}

impl Token {
    /// Renews `REDDIT_TOKEN_RENEW_MARGIN_SECS` before expiry, or at half-life
    /// for a token shorter than twice the margin, so a short token is not
    /// renewed on every request. Lifetime is clamped so `Instant` math cannot
    /// overflow on a nonsense `expires_in`.
    fn issued(access_token: String, expires_in: u64, now: Instant) -> Self {
        let lifetime = expires_in.min(config::REDDIT_TOKEN_MAX_LIFETIME_SECS);
        let margin = config::REDDIT_TOKEN_RENEW_MARGIN_SECS.min(lifetime / 2);
        Self {
            access_token,
            expires_at: now + Duration::from_secs(lifetime),
            renew_at: now + Duration::from_secs(lifetime - margin),
        }
    }

    /// After a failed renewal, keep using this token and try again later
    /// rather than on every request; never past expiry.
    fn postpone_renewal(&mut self, now: Instant) {
        let retry_at = now + Duration::from_secs(config::REDDIT_TOKEN_RENEW_RETRY_SECS);
        self.renew_at = retry_at.min(self.expires_at);
    }
}

/// Persisted form of a session. No `Debug` on purpose: a stray `{:?}` must not
/// print the bearer token. Written only to a 0600 file.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedRedditSession {
    pub(crate) user_agent: String,
    pub(crate) vendor_id: String,
    pub(crate) access_token: String,
    pub(crate) expires_at_unix: i64,
}

pub struct RedditSession {
    device: Device,
    token: Option<Token>,
}

impl RedditSession {
    pub fn new() -> Self {
        Self {
            device: Device::random(),
            token: None,
        }
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// The device and its token with a wall-clock expiry. `None` without a
    /// token: an identity alone is worth nothing.
    pub(crate) fn persist(&self) -> Option<PersistedRedditSession> {
        let token = self.token.as_ref()?;
        let remaining = token.expires_at.saturating_duration_since(Instant::now());
        Some(PersistedRedditSession {
            user_agent: self.device.user_agent.clone(),
            vendor_id: self.device.vendor_id.clone(),
            access_token: token.access_token.clone(),
            expires_at_unix: Utc::now()
                .timestamp()
                .saturating_add(remaining.as_secs() as i64),
        })
    }

    /// Rebuilds a session from disk. `None` once the token expired: the caller
    /// starts a fresh identity instead of trusting a dead token.
    pub(crate) fn restore(persisted: PersistedRedditSession) -> Option<Self> {
        // Saturating: a hand-edited or corrupted extreme must not overflow.
        let remaining = persisted
            .expires_at_unix
            .saturating_sub(Utc::now().timestamp());
        if remaining <= 0 {
            return None;
        }

        Some(Self {
            device: Device {
                user_agent: persisted.user_agent,
                vendor_id: persisted.vendor_id,
            },
            token: Some(Token::issued(
                persisted.access_token,
                remaining as u64,
                Instant::now(),
            )),
        })
    }

    /// Requests a token when none is held or the current one is due for
    /// renewal. A renewal comes from a new device, as a fresh install would;
    /// if it fails while the current token is still valid, the current token
    /// and device stay in use.
    pub async fn ensure_token(&mut self, client: &Client) -> Result<(), FetchError> {
        let now = Instant::now();
        if !needs_renewal(self.token.as_ref(), now) {
            return Ok(());
        }
        let device = if self.token.is_some() {
            Device::random()
        } else {
            self.device.clone()
        };

        match request_token(client, &device).await {
            Ok(response) => {
                info!(
                    expires_in_secs = response.expires_in,
                    user_agent = %device.user_agent,
                    "reddit: new token"
                );
                self.device = device;
                self.token = Some(Token::issued(
                    response.access_token,
                    response.expires_in,
                    Instant::now(),
                ));
                Ok(())
            }
            Err(e) => match &mut self.token {
                Some(token) if token.expires_at > now => {
                    warn!(error = %error_chain(&e), "reddit: token renewal failed, keeping current token");
                    token.postpone_renewal(now);
                    Ok(())
                }
                _ => Err(e),
            },
        }
    }

    /// After a 401: the token is dead, and so is the identity it belonged to.
    pub fn invalidate(&mut self) {
        self.token = None;
        self.device = Device::random();
    }

    /// Device headers plus the bearer token, when one is held.
    pub fn authorize(&self, request: RequestBuilder) -> RequestBuilder {
        let request = self.device.headers(request);
        match &self.token {
            Some(token) => request.bearer_auth(&token.access_token),
            None => request,
        }
    }
}

impl Device {
    fn headers(&self, request: RequestBuilder) -> RequestBuilder {
        request
            .header(USER_AGENT, &self.user_agent)
            .header("client-vendor-id", &self.vendor_id)
            .header("x-reddit-retry", "algo=no-retries")
    }
}

async fn request_token(client: &Client, device: &Device) -> Result<TokenResponse, FetchError> {
    Ok(device
        .headers(client.post(config::REDDIT_TOKEN_URL))
        .basic_auth(config::REDDIT_ANDROID_CLIENT_ID, Some(""))
        .header(CONTENT_TYPE, "application/json")
        .body(r#"{"scopes":["*","email","pii"]}"#)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

fn needs_renewal(token: Option<&Token>, now: Instant) -> bool {
    token.is_none_or(|t| now >= t.renew_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::tests::test_clients;

    #[test]
    fn day_token_renews_one_margin_before_expiry() {
        let now = Instant::now();
        let token = Token::issued("t".into(), 86_400, now);
        let renew_after = 86_400 - config::REDDIT_TOKEN_RENEW_MARGIN_SECS;
        assert!(!needs_renewal(Some(&token), now));
        assert!(!needs_renewal(
            Some(&token),
            now + Duration::from_secs(renew_after - 1)
        ));
        assert!(needs_renewal(
            Some(&token),
            now + Duration::from_secs(renew_after)
        ));
    }

    #[test]
    fn short_token_renews_at_half_life() {
        let now = Instant::now();
        let token = Token::issued("t".into(), 3600, now);
        assert!(!needs_renewal(Some(&token), now + Duration::from_secs(1)));
        assert!(needs_renewal(Some(&token), now + Duration::from_secs(1800)));
    }

    #[test]
    fn huge_expiry_is_clamped() {
        let now = Instant::now();
        let token = Token::issued("t".into(), u64::MAX, now);
        assert_eq!(
            token.expires_at - now,
            Duration::from_secs(config::REDDIT_TOKEN_MAX_LIFETIME_SECS)
        );
    }

    #[test]
    fn failed_renewal_postpones_but_not_past_expiry() {
        let now = Instant::now();
        let mut token = Token::issued("t".into(), 86_400, now);
        let due = token.renew_at;
        token.postpone_renewal(due);
        assert_eq!(
            token.renew_at,
            due + Duration::from_secs(config::REDDIT_TOKEN_RENEW_RETRY_SECS)
        );

        let near_end = token.expires_at - Duration::from_secs(1);
        token.postpone_renewal(near_end);
        assert_eq!(token.renew_at, token.expires_at);
    }

    #[test]
    fn session_persist_restore_keeps_token_and_device() {
        let mut session = RedditSession::new();
        session.token = Some(Token::issued("token".into(), 86_400, Instant::now()));

        let persisted = session.persist().expect("a token is persisted");
        assert_eq!(persisted.access_token, "token");
        assert_eq!(persisted.user_agent, session.device.user_agent);
        assert_eq!(persisted.vendor_id, session.device.vendor_id);

        let restored = RedditSession::restore(persisted).expect("unexpired token restores");
        assert!(restored.has_token());
        assert_eq!(restored.device.user_agent, session.device.user_agent);
        assert_eq!(restored.device.vendor_id, session.device.vendor_id);
        let remaining = restored.token.as_ref().unwrap().expires_at - Instant::now();
        assert!(remaining.as_secs() > 86_000, "{remaining:?}");
    }

    #[test]
    fn expired_session_is_discarded() {
        let persisted = PersistedRedditSession {
            user_agent: "Reddit/Version 2024.22.1/Build 1/Android 14".into(),
            vendor_id: "vendor".into(),
            access_token: "token".into(),
            expires_at_unix: chrono::Utc::now().timestamp() - 1,
        };

        assert!(RedditSession::restore(persisted).is_none());
    }

    #[test]
    fn extreme_expiry_is_not_restored() {
        let persisted = PersistedRedditSession {
            user_agent: "Reddit/Version 2024.22.1/Build 1/Android 14".into(),
            vendor_id: "vendor".into(),
            access_token: "token".into(),
            expires_at_unix: i64::MIN,
        };

        assert!(RedditSession::restore(persisted).is_none());
    }

    #[test]
    fn persist_without_token_is_none() {
        assert!(RedditSession::new().persist().is_none());
    }

    #[test]
    fn invalidate_drops_token_and_device() {
        let mut session = RedditSession::new();
        session.token = Some(Token::issued("t".into(), 86_400, Instant::now()));
        let old_vendor_id = session.device.vendor_id.clone();

        session.invalidate();

        assert!(session.token.is_none());
        assert_ne!(session.device.vendor_id, old_vendor_id);
    }

    #[tokio::test]
    #[ignore = "live HTTP"]
    async fn token_request_live() {
        if !crate::config::reddit_scraper_enabled() {
            return;
        }
        let mut session = RedditSession::new();
        session.ensure_token(&test_clients().reddit).await.unwrap();
        assert!(session.token.is_some());
    }
}
