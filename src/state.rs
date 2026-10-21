//! On-disk cache of the last good snapshot and the Reddit session, so a restart
//! serves real panels instead of the loading page.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::DateTime;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::cache::DataSnapshot;
use crate::config;
use crate::providers::github::TrendingRepo;
use crate::providers::hackernews::{HnStory, PersistedHnStory};
use crate::providers::reddit::{PersistedRedditPost, RedditPost};
use crate::providers::reddit_auth::{PersistedRedditSession, RedditSession};

pub const FEED_FILE: &str = "feed.json";
pub const SESSION_FILE: &str = "reddit-session.json";

/// Each file versions independently: a session shape change must not discard a
/// good feed, and vice versa. A mismatch starts cold for that file only.
const FEED_FORMAT_VERSION: u32 = 1;
const SESSION_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FeedFile {
    version: u32,
    last_fetched: i64,
    hn_pages: HashMap<String, Vec<PersistedHnStory>>,
    gh_trending: Vec<GitHubPage>,
    reddit_feed: HashMap<String, Vec<PersistedRedditPost>>,
}

/// GitHub's live key is `(period, language)`, which JSON cannot use as an
/// object key, so each key becomes an entry.
#[derive(Serialize, Deserialize)]
struct GitHubPage {
    period: String,
    language: String,
    repos: Vec<TrendingRepo>,
}

#[derive(Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    #[serde(flatten)]
    session: PersistedRedditSession,
}

#[derive(Clone)]
pub struct StateStore {
    dir: PathBuf,
}

impl StateStore {
    pub fn from_config() -> Self {
        let dir = config::state_dir();
        tracing::debug!(path = %dir.display(), "state: directory");
        Self::new(dir)
    }

    fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, file: &str) -> PathBuf {
        self.dir.join(file)
    }

    pub fn load_feed(&self) -> Option<DataSnapshot> {
        let path = self.path(FEED_FILE);
        let file: FeedFile = read_json(&path)?;
        if file.version != FEED_FORMAT_VERSION {
            tracing::warn!(
                path = %path.display(),
                version = file.version,
                expected = FEED_FORMAT_VERSION,
                "state: ignoring feed from another format version"
            );
            return None;
        }

        let Some(last_fetched) = DateTime::from_timestamp(file.last_fetched, 0) else {
            tracing::warn!(
                path = %path.display(),
                value = file.last_fetched,
                "state: ignoring feed with an invalid timestamp"
            );
            return None;
        };
        // An epoch timestamp means no cycle ever succeeded; there is nothing to show.
        if last_fetched.timestamp() <= 0 {
            return None;
        }

        tracing::info!(
            path = %path.display(),
            last_fetched = %last_fetched,
            "state: restored feed"
        );

        Some(DataSnapshot {
            hn_pages: file
                .hn_pages
                .into_iter()
                .map(|(page, stories)| (page, stories.into_iter().map(HnStory::from).collect()))
                .collect(),
            gh_trending: file
                .gh_trending
                .into_iter()
                .map(|page| ((page.period, page.language), page.repos))
                .collect(),
            reddit_feed: file
                .reddit_feed
                .into_iter()
                .map(|(subreddit, posts)| {
                    (subreddit, posts.into_iter().map(RedditPost::from).collect())
                })
                .collect(),
            last_fetched,
        })
    }

    pub fn save_feed(&self, data: &DataSnapshot) {
        let last_fetched = data.last_fetched.timestamp();
        if last_fetched <= 0 {
            return;
        }

        let file = FeedFile {
            version: FEED_FORMAT_VERSION,
            last_fetched,
            hn_pages: data
                .hn_pages
                .iter()
                .map(|(page, stories)| {
                    (
                        page.clone(),
                        stories.iter().map(PersistedHnStory::from).collect(),
                    )
                })
                .collect(),
            gh_trending: data
                .gh_trending
                .iter()
                .map(|((period, language), repos)| GitHubPage {
                    period: period.clone(),
                    language: language.clone(),
                    repos: repos.clone(),
                })
                .collect(),
            reddit_feed: data
                .reddit_feed
                .iter()
                .map(|(subreddit, posts)| {
                    (
                        subreddit.clone(),
                        posts.iter().map(PersistedRedditPost::from).collect(),
                    )
                })
                .collect(),
        };

        let path = self.path(FEED_FILE);
        match write_json(&path, &file) {
            Ok(()) => tracing::debug!(path = %path.display(), "state: feed saved"),
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "state: feed save failed")
            }
        }
    }

    pub fn load_reddit_session(&self) -> Option<PersistedRedditSession> {
        let path = self.path(SESSION_FILE);
        let file: SessionFile = read_json(&path)?;
        if file.version != SESSION_FORMAT_VERSION {
            tracing::warn!(
                path = %path.display(),
                version = file.version,
                expected = SESSION_FORMAT_VERSION,
                "state: ignoring session from another format version"
            );
            return None;
        }

        Some(file.session)
    }

    pub fn save_reddit_session(&self, session: &RedditSession) {
        let Some(persisted) = session.persist() else {
            self.clear_session();
            return;
        };

        if let Err(error) = self.write_session(&persisted) {
            let path = self.path(SESSION_FILE);
            tracing::warn!(path = %path.display(), error = %error, "state: session save failed");
        }
    }

    /// A rejected or absent identity must not survive on disk and be restored.
    fn clear_session(&self) {
        let path = self.path(SESSION_FILE);
        match fs::remove_file(&path) {
            Ok(()) => tracing::info!("state: session cleared"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                path = %path.display(),
                error = %error,
                "state: session clear failed"
            ),
        }
    }

    fn write_session(&self, session: &PersistedRedditSession) -> io::Result<()> {
        write_json(
            &self.path(SESSION_FILE),
            &SessionFile {
                version: SESSION_FORMAT_VERSION,
                session: session.clone(),
            },
        )
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "state: cannot read file");
            return None;
        }
    };

    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "state: ignoring unreadable file");
            None
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_atomic(path, &bytes)
}

/// Same-directory temp file, fsync, then rename. `rename(2)` replaces the
/// destination atomically, so a kill leaves either the old or the new complete
/// file. The directory is not fsynced: power loss can only lose the newest
/// save, never corrupt one.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = tmp_path(path);
    let result = write_tmp(&tmp, bytes).and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_tmp(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // A temp file left by a crash could be a symlink or carry a looser mode;
    // removing it makes create_new produce a fresh 0600 file.
    let _ = fs::remove_file(path);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Create 0600; umask can only remove permission bits, never add them.
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::github::GhTrending;
    use crate::providers::hackernews::HnPages;
    use crate::providers::hackernews::test_story;
    use crate::providers::reddit::RedditFeed;
    use crate::providers::reddit::test_post;
    use chrono::{TimeZone, Utc};
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tty1-state-{name}-{}", fastrand::u64(..)));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn repo(name: &str) -> TrendingRepo {
        TrendingRepo {
            author: "alice".into(),
            name: name.into(),
            url: format!("https://github.com/alice/{name}"),
            description: "a repo".into(),
            language: Some("Rust".into()),
            language_color: Some("#dea584".into()),
            stars: 10,
            forks: 2,
            period_stars: "5 stars today".into(),
        }
    }

    fn sample_data() -> DataSnapshot {
        DataSnapshot {
            hn_pages: HnPages::from([("top".to_string(), vec![test_story("a story")])]),
            gh_trending: GhTrending::from([(
                ("daily".to_string(), "Rust".to_string()),
                vec![repo("tty1")],
            )]),
            reddit_feed: RedditFeed::from([
                ("rust".to_string(), vec![test_post("rust", 10)]),
                ("all".to_string(), vec![test_post("rust", 10)]),
            ]),
            last_fetched: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
        }
    }

    #[test]
    fn feed_round_trips() {
        let dir = temp_dir("round-trip");
        let store = StateStore::new(dir.clone());
        store.save_feed(&sample_data());

        let restored = store.load_feed().expect("feed restored");

        assert_eq!(restored.last_fetched, sample_data().last_fetched);
        let story = &restored.hn_pages["top"][0];
        assert_eq!(story.title, "a story");
        assert_eq!(story.hn_url, "https://news.ycombinator.com/item?id=1");
        assert_eq!(story.domain.as_deref(), Some("example.com"));

        let repos = &restored.gh_trending[&("daily".to_string(), "Rust".to_string())];
        assert_eq!(repos[0].name, "tty1");
        assert_eq!(repos[0].period_stars, "5 stars today");

        assert_eq!(restored.reddit_feed["rust"][0].score, 10);
        assert_eq!(restored.reddit_feed["all"][0].subreddit, "rust");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_version_is_ignored() {
        let dir = temp_dir("version");
        let store = StateStore::new(dir.clone());
        fs::write(
            store.path(FEED_FILE),
            r#"{"version":999,"last_fetched":1700000000,"hn_pages":{},"gh_trending":[],"reddit_feed":{}}"#,
        )
        .unwrap();

        assert!(store.load_feed().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_timestamp_is_ignored() {
        let dir = temp_dir("timestamp");
        let store = StateStore::new(dir.clone());
        fs::write(
            store.path(FEED_FILE),
            r#"{"version":1,"last_fetched":9223372036854775807,"hn_pages":{},"gh_trending":[],"reddit_feed":{}}"#,
        )
        .unwrap();

        assert!(store.load_feed().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_ignored() {
        let dir = temp_dir("corrupt");
        let store = StateStore::new(dir.clone());
        fs::write(store.path(FEED_FILE), "{not json").unwrap();

        assert!(store.load_feed().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_snapshot_is_not_saved() {
        let dir = temp_dir("empty");
        let store = StateStore::new(dir.clone());
        let mut data = sample_data();
        data.last_fetched = DateTime::UNIX_EPOCH;

        store.save_feed(&data);

        assert!(!store.path(FEED_FILE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_replaces_and_leaves_no_temp_file() {
        let dir = temp_dir("replace");
        let store = StateStore::new(dir.clone());
        let mut data = sample_data();
        store.save_feed(&data);

        data.last_fetched = Utc.timestamp_opt(1_700_000_100, 0).single().unwrap();
        store.save_feed(&data);

        let restored = store.load_feed().expect("feed restored");
        assert_eq!(restored.last_fetched.timestamp(), 1_700_000_100);
        assert!(!store.path("feed.json.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_file_round_trips() {
        let dir = temp_dir("session");
        let store = StateStore::new(dir.clone());
        let session = PersistedRedditSession {
            user_agent: "Reddit/Version 2024.22.1/Build 1/Android 14".into(),
            vendor_id: "vendor".into(),
            access_token: "token".into(),
            expires_at_unix: Utc::now().timestamp() + 3600,
        };

        store.write_session(&session).unwrap();

        let loaded = store.load_reddit_session().expect("session loaded");
        assert_eq!(loaded.user_agent, session.user_agent);
        assert_eq!(loaded.vendor_id, session.vendor_id);
        assert_eq!(loaded.access_token, session.access_token);
        assert_eq!(loaded.expires_at_unix, session.expires_at_unix);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_without_token_clears_the_file() {
        let dir = temp_dir("session-clear");
        let store = StateStore::new(dir.clone());
        let session = PersistedRedditSession {
            user_agent: "Reddit/Version 2024.22.1/Build 1/Android 14".into(),
            vendor_id: "vendor".into(),
            access_token: "token".into(),
            expires_at_unix: Utc::now().timestamp() + 3600,
        };
        store.write_session(&session).unwrap();
        assert!(store.path(SESSION_FILE).exists());

        store.save_reddit_session(&RedditSession::new());

        assert!(!store.path(SESSION_FILE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn stale_temp_file_is_replaced_with_a_private_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("stale-tmp");
        let store = StateStore::new(dir.clone());
        let tmp = store.path("feed.json.tmp");
        fs::write(&tmp, b"stale").unwrap();
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)).unwrap();

        store.save_feed(&sample_data());

        assert!(!tmp.exists(), "stale temp file removed");
        let mode = fs::metadata(store.path(FEED_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "mode not inherited from stale temp");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn feed_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("mode");
        let store = StateStore::new(dir.clone());
        store.save_feed(&sample_data());

        let mode = fs::metadata(store.path(FEED_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }
}
