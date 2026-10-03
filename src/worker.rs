use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::cache::{DataSnapshot, HtmlSnapshot, SharedData, SharedHtml};
use crate::client::Client;
use crate::config;
use crate::providers::reddit::{RedditFeed, RedditFetch};
use crate::providers::reddit_auth::RedditSession;
use crate::providers::{FetchError, error_chain, github, hackernews, reddit};

/// Two loops write the shared snapshot: HN + GitHub, and Reddit. Each holds
/// this across load-merge-store so neither overwrites the other's update.
/// Held only for synchronous work, never across an await.
static SNAPSHOT_WRITE_LOCK: Mutex<()> = Mutex::new(());

/// HN and GitHub. Each provider fails independently — a single provider
/// outage never blocks the others. Reddit runs in `run_reddit_scraper`.
pub async fn run_scraper(
    data: SharedData,
    html: SharedHtml,
    client: Client,
    interval: Duration,
    story_count: usize,
) {
    loop {
        fetch_and_update(&data, &client, story_count).await;
        rebuild_html(&data, &html).await;
        let sleep_dur = jittered_interval(interval, config::SCRAPE_JITTER_SECS);
        tracing::debug!(sleep_secs = sleep_dur.as_secs(), "next scrape");
        tokio::time::sleep(sleep_dur).await;
    }
}

/// Reddit on its own, shorter cycle: each pass fetches a random subset, so a
/// shorter interval keeps every subreddit fresh while spreading the requests.
pub async fn run_reddit_scraper(data: SharedData, html: SharedHtml, client: Client) {
    let mut session = RedditSession::new();
    let interval = Duration::from_secs(config::REDDIT_SCRAPE_INTERVAL_SECS);
    loop {
        fetch_and_update_reddit(&data, &client, &mut session).await;
        rebuild_html(&data, &html).await;
        let sleep_dur = jittered_interval(interval, config::REDDIT_SCRAPE_JITTER_SECS);
        tracing::debug!(sleep_secs = sleep_dur.as_secs(), "next reddit scrape");
        tokio::time::sleep(sleep_dur).await;
    }
}

/// `interval` ± `jitter_secs`, uniform.
fn jittered_interval(interval: Duration, jitter_secs: u64) -> Duration {
    let jitter = Duration::from_secs(fastrand::u64(0..=jitter_secs * 2));
    interval - Duration::from_secs(jitter_secs) + jitter
}

/// Separate from scraper: re-renders HTML every minute so relative timestamps ("3h ago")
/// stay fresh between data fetches.
pub async fn run_html_refresher(data: SharedData, html: SharedHtml, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        if data.load().last_fetched.timestamp() > 0 {
            rebuild_html(&data, &html).await;
        }
    }
}

async fn rebuild_html(data: &SharedData, html: &SharedHtml) {
    let snap = data.load_full();
    let start = Instant::now();
    // spawn_blocking: Maud rendering is CPU-bound and would block the async runtime.
    match tokio::task::spawn_blocking(move || HtmlSnapshot::from_data(&snap)).await {
        Ok(Some(new_html)) => {
            tracing::debug!(elapsed_ms = start.elapsed().as_millis(), "html rendered");
            html.store(Arc::new(new_html));
        }
        Ok(None) => tracing::error!("failed to build HTML snapshot, keeping previous"),
        Err(e) => tracing::error!(error = %e, "HTML rebuild task panicked, keeping previous"),
    }
}

/// Take fresh data where the fetch succeeded, keep the previous snapshot's value
/// where it failed. An empty `Ok` is real data and is accepted: only an error
/// falls back. The merged map holds exactly the keys of `fresh`, so a key
/// removed from config drops its retained data.
///
/// Returns the merged map and the number of keys that came back fresh.
fn merge_keyed<K, V>(
    fresh: std::collections::HashMap<K, Result<Vec<V>, FetchError>>,
    current: &std::collections::HashMap<K, Vec<V>>,
    label: &str,
) -> (std::collections::HashMap<K, Vec<V>>, usize)
where
    K: std::hash::Hash + Eq + Clone + std::fmt::Debug,
    V: Clone,
{
    let mut merged = std::collections::HashMap::with_capacity(fresh.len());
    let mut fresh_count = 0;

    for (key, result) in fresh {
        match result {
            Ok(items) => {
                fresh_count += 1;
                merged.insert(key, items);
            }
            Err(e) => {
                let retained = current.get(&key).cloned().unwrap_or_default();
                tracing::warn!(
                    provider = label,
                    key = ?key,
                    error = %error_chain(&e),
                    retained = retained.len(),
                    "fetch failed, keeping previous"
                );
                merged.insert(key, retained);
            }
        }
    }

    (merged, fresh_count)
}

async fn fetch_and_update(data: &SharedData, client: &Client, story_count: usize) {
    tracing::info!("scrape cycle starting");

    let start = Instant::now();

    // Both providers fetch concurrently — each handles its own rate limiting internally.
    let (hn_fresh, gh_fresh) = tokio::join!(
        hackernews::fetch_all_pages(client, story_count),
        github::fetch_all_trending(client),
    );

    let _write = SNAPSHOT_WRITE_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let current = data.load();

    let (hn_pages, hn_ok) = merge_keyed(hn_fresh, &current.hn_pages, "hn");
    let (gh_trending, gh_ok) = merge_keyed(gh_fresh, &current.gh_trending, "github");

    // Per-key retention makes an outage invisible in the UI, so the log is the
    // only signal that a whole source is down.
    for (label, ok, total) in [
        ("hn", hn_ok, hn_pages.len()),
        ("github", gh_ok, gh_trending.len()),
    ] {
        if total > 0 && ok == 0 {
            tracing::error!(provider = label, keys = total, "every key failed");
        }
    }

    // Do not advance the clock on a total failure: the footer would claim the
    // data is fresh when nothing changed.
    let last_fetched = if hn_ok + gh_ok > 0 {
        Utc::now()
    } else {
        current.last_fetched
    };

    tracing::info!(
        hn_fresh = hn_ok,
        hn_stale = hn_pages.len() - hn_ok,
        gh_fresh = gh_ok,
        gh_stale = gh_trending.len() - gh_ok,
        elapsed_secs = start.elapsed().as_secs(),
        "data cache updated"
    );

    data.store(Arc::new(DataSnapshot {
        hn_pages,
        gh_trending,
        reddit_feed: current.reddit_feed.clone(),
        last_fetched,
    }));
}

async fn fetch_and_update_reddit(data: &SharedData, client: &Client, session: &mut RedditSession) {
    let start = Instant::now();

    let subreddits = reddit::plan_cycle(&data.load().reddit_feed, &mut fastrand::Rng::new());
    let fresh = reddit::fetch_reddit_feed(client, session, &subreddits).await;
    let requested = fresh.len();

    let _write = SNAPSHOT_WRITE_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let current = data.load();

    let (reddit_feed, reddit_ok) =
        merge_reddit(fresh, &current.reddit_feed, config::REDDIT_SUBREDDITS);

    if requested > 0 && reddit_ok == 0 {
        tracing::error!(provider = "reddit", keys = requested, "every key failed");
    }

    let last_fetched = if reddit_ok > 0 {
        Utc::now()
    } else {
        current.last_fetched
    };

    tracing::info!(
        reddit_fresh = reddit_ok,
        reddit_failed = requested - reddit_ok,
        reddit_skipped = config::REDDIT_SUBREDDITS.len() - requested,
        elapsed_secs = start.elapsed().as_secs(),
        "reddit cache updated"
    );

    data.store(Arc::new(DataSnapshot {
        hn_pages: current.hn_pages.clone(),
        gh_trending: current.gh_trending.clone(),
        reddit_feed,
        last_fetched,
    }));
}

/// `merge_keyed` over the fetched subreddits, then every configured subreddit
/// this cycle skipped keeps its previous posts. Unconfigured keys drop. The
/// "all" view is rebuilt last so it draws on retained data too.
fn merge_reddit(
    fresh: RedditFetch,
    current: &RedditFeed,
    configured: &[&str],
) -> (RedditFeed, usize) {
    let (mut merged, fresh_count) = merge_keyed(fresh, current, "reddit");
    for sub in configured {
        if let Entry::Vacant(slot) = merged.entry(sub.to_lowercase()) {
            let previous = current.get(slot.key()).cloned().unwrap_or_default();
            slot.insert(previous);
        }
    }
    let all = reddit::build_all_view(&merged);
    merged.insert(config::FILTER_ALL.to_string(), all);
    (merged, fresh_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn current() -> HashMap<&'static str, Vec<u8>> {
        HashMap::from([("a", vec![1, 2]), ("b", vec![3])])
    }

    #[test]
    fn fresh_data_wins() {
        let fresh = HashMap::from([("a", Ok(vec![9]))]);
        let (merged, ok) = merge_keyed(fresh, &current(), "t");
        assert_eq!(merged["a"], vec![9]);
        assert_eq!(ok, 1);
    }

    #[test]
    fn empty_ok_is_accepted() {
        let fresh = HashMap::from([("a", Ok(Vec::new()))]);
        let (merged, ok) = merge_keyed(fresh, &current(), "t");
        assert!(merged["a"].is_empty());
        assert_eq!(ok, 1);
    }

    #[test]
    fn error_keeps_previous() {
        let fresh = HashMap::from([("a", Err(FetchError::EmptyPage))]);
        let (merged, ok) = merge_keyed(fresh, &current(), "t");
        assert_eq!(merged["a"], vec![1, 2]);
        assert_eq!(ok, 0);
    }

    #[test]
    fn error_on_unknown_key_yields_empty() {
        let fresh = HashMap::from([("z", Err(FetchError::EmptyPage))]);
        let (merged, _) = merge_keyed(fresh, &current(), "t");
        assert!(merged["z"].is_empty());
    }

    fn posts(subreddit: &str, score: i64) -> Vec<reddit::RedditPost> {
        vec![reddit::RedditPost {
            title: "t".into(),
            url: String::new(),
            permalink: String::new(),
            subreddit: subreddit.into(),
            score,
            author: "a".into(),
            created_at: 0.0,
            comment_count: 0,
            domain: String::new(),
            is_self: false,
            stickied: false,
        }]
    }

    fn previous_feed() -> RedditFeed {
        RedditFeed::from([
            ("rust".to_string(), posts("rust", 1)),
            ("zig".to_string(), posts("zig", 2)),
            ("removed".to_string(), posts("removed", 3)),
            ("all".to_string(), posts("stale", 9999)),
        ])
    }

    #[test]
    fn reddit_merge_keeps_skipped_keys() {
        let fresh = RedditFetch::from([("rust".to_string(), Ok(posts("rust", 10)))]);
        let (merged, ok) = merge_reddit(fresh, &previous_feed(), &["rust", "zig", "go"]);
        assert_eq!(ok, 1);
        assert_eq!(merged["rust"][0].score, 10);
        assert_eq!(
            merged["zig"][0].score, 2,
            "skipped key keeps previous posts"
        );
        assert!(
            merged["go"].is_empty(),
            "never-fetched key is present and empty"
        );
    }

    #[test]
    fn reddit_merge_drops_unconfigured() {
        let fresh = RedditFetch::new();
        let (merged, _) = merge_reddit(fresh, &previous_feed(), &["rust"]);
        assert!(!merged.contains_key("removed"));
        assert!(!merged.contains_key("zig"));
    }

    #[test]
    fn reddit_merge_rebuilds_all_view() {
        let fresh = RedditFetch::from([("rust".to_string(), Ok(posts("rust", 10)))]);
        let (merged, _) = merge_reddit(fresh, &previous_feed(), &["rust", "zig"]);
        let scores: Vec<i64> = merged["all"].iter().map(|p| p.score).collect();
        assert_eq!(scores, vec![10, 2]);
    }

    #[test]
    fn removed_key_disappears() {
        let fresh = HashMap::from([("a", Ok(vec![9]))]);
        let (merged, _) = merge_keyed(fresh, &current(), "t");
        assert!(!merged.contains_key("b"));
    }
}
