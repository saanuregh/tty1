use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::cache::{
    DataSnapshot, FragmentSnapshot, HtmlSnapshot, SharedData, SharedFragments, SharedHtml,
};
use crate::client::Client;
use crate::config;
use crate::providers::reddit::{RedditFeed, RedditFetch};
use crate::providers::reddit_auth::RedditSession;
use crate::providers::{FetchError, error_chain, github, hackernews, reddit};
use crate::render::View;
use crate::state::StateStore;

/// Two loops write the shared snapshot: HN + GitHub, and Reddit. Each holds
/// this across load-merge-store-save so neither overwrites the other's update.
/// Held only for synchronous work, never across an await.
static SNAPSHOT_WRITE_LOCK: Mutex<()> = Mutex::new(());

/// HN and GitHub. Each provider fails independently — a single provider
/// outage never blocks the others. Reddit runs in `run_reddit_scraper`.
pub async fn run_scraper(
    data: SharedData,
    html: SharedHtml,
    fragments: SharedFragments,
    client: Client,
    interval: Duration,
    story_count: usize,
    store: StateStore,
) {
    loop {
        fetch_and_update(&data, &html, &fragments, &client, story_count, &store).await;
        let sleep_dur = jittered_interval(interval, config::SCRAPE_JITTER_SECS);
        tracing::debug!(sleep_secs = sleep_dur.as_secs(), "next scrape");
        tokio::time::sleep(sleep_dur).await;
    }
}

/// Reddit on its own, shorter cycle: each pass fetches a random subset, so a
/// shorter interval keeps every subreddit fresh while spreading the requests.
pub async fn run_reddit_scraper(
    data: SharedData,
    html: SharedHtml,
    fragments: SharedFragments,
    client: Client,
    store: StateStore,
) {
    let mut session = match store.load_reddit_session() {
        Some(persisted) => RedditSession::restore(persisted).unwrap_or_else(|| {
            tracing::info!("reddit: persisted session expired, starting fresh");
            RedditSession::new()
        }),
        None => RedditSession::new(),
    };
    let interval = Duration::from_secs(config::REDDIT_SCRAPE_INTERVAL_SECS);
    loop {
        if fetch_and_update_reddit(&data, &client, &mut session, &store).await {
            rebuild_snapshots(&data, &html, &fragments).await;
        }
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

/// Separate from scraper: re-renders the shell every minute so server-rendered
/// relative timestamps stay fresh for no-JS/first-paint clients. Fragments are
/// immutable per data version and are refreshed in the browser after insertion.
pub async fn run_html_refresher(data: SharedData, html: SharedHtml, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        if data.load().last_fetched.timestamp() > 0 {
            rebuild_shell(&data, &html).await;
        }
    }
}

/// Data changes rebuild both stores; fragments swap first so a poll that sees
/// the new shell version can already fetch matching fragments.
async fn rebuild_snapshots(data: &SharedData, html: &SharedHtml, fragments: &SharedFragments) {
    let snap = data.load_full();
    let check = snap.clone();
    let start = Instant::now();
    let built = tokio::task::spawn_blocking(move || {
        let fragments = FragmentSnapshot::from_data(&snap);
        let html = HtmlSnapshot::from_data(&snap, &View::default());
        (fragments, html)
    })
    .await;

    match built {
        Ok((fragments_snapshot, Some(html_snapshot))) => {
            tracing::debug!(
                elapsed_ms = start.elapsed().as_millis(),
                fragments = fragments_snapshot.fragments.len(),
                "html and fragments rendered"
            );
            let _write = SNAPSHOT_WRITE_LOCK
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !Arc::ptr_eq(&data.load_full(), &check) {
                tracing::debug!("snapshots superseded, keeping newer data");
                return;
            }
            fragments.store(Arc::new(fragments_snapshot));
            html.store(Arc::new(html_snapshot));
        }
        Ok((fragments_snapshot, None)) => {
            tracing::error!("failed to build HTML snapshot, keeping previous");
            let _write = SNAPSHOT_WRITE_LOCK
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if Arc::ptr_eq(&data.load_full(), &check) {
                fragments.store(Arc::new(fragments_snapshot));
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "HTML rebuild task panicked, keeping previous");
        }
    }
}

/// Time-only refresh: fragments are immutable per data version, so only the
/// shell needs new relative timestamps.
async fn rebuild_shell(data: &SharedData, html: &SharedHtml) {
    let snap = data.load_full();
    let check = snap.clone();
    let start = Instant::now();
    // spawn_blocking: Maud rendering is CPU-bound and would block the async runtime.
    match tokio::task::spawn_blocking(move || HtmlSnapshot::from_data(&snap, &View::default()))
        .await
    {
        Ok(Some(new_html)) => {
            tracing::debug!(elapsed_ms = start.elapsed().as_millis(), "html rendered");
            let _write = SNAPSHOT_WRITE_LOCK
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if Arc::ptr_eq(&data.load_full(), &check) {
                html.store(Arc::new(new_html));
            }
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

async fn fetch_and_update(
    data: &SharedData,
    html: &SharedHtml,
    fragments: &SharedFragments,
    client: &Client,
    story_count: usize,
    store: &StateStore,
) {
    tracing::info!("scrape cycle starting");

    // Publish and render each provider as it finishes. HN is ready in seconds,
    // GitHub paces 54 requests, so waiting for both would leave the page on the
    // loading screen for over a minute.
    tokio::join!(
        async {
            let fresh = hackernews::fetch_all_pages(client, story_count).await;
            if publish_hn(data, fresh, store) {
                rebuild_snapshots(data, html, fragments).await;
            }
        },
        async {
            let fresh = github::fetch_all_trending(client).await;
            if publish_gh(data, fresh, store) {
                rebuild_snapshots(data, html, fragments).await;
            }
        },
    );
}

/// Strictly increasing, so two publishes in one wall-clock second still get
/// distinct versions and ETags.
fn next_fetched(current: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    let now = Utc::now();
    if now.timestamp() > current.timestamp() {
        now
    } else {
        current + chrono::TimeDelta::seconds(1)
    }
}

fn publish_hn(data: &SharedData, fresh: hackernews::HnFetch, store: &StateStore) -> bool {
    let _write = SNAPSHOT_WRITE_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let current = data.load();
    let (hn_pages, hn_ok) = merge_keyed(fresh, &current.hn_pages, "hn");

    if !hn_pages.is_empty() && hn_ok == 0 {
        tracing::error!(provider = "hn", keys = hn_pages.len(), "every key failed");
        return false;
    }

    tracing::info!(
        hn_fresh = hn_ok,
        hn_stale = hn_pages.len() - hn_ok,
        "hn cache updated"
    );
    let last_fetched = next_fetched(current.last_fetched);

    let next = Arc::new(DataSnapshot {
        hn_pages,
        gh_trending: current.gh_trending.clone(),
        reddit_feed: current.reddit_feed.clone(),
        last_fetched,
    });
    data.store(next.clone());
    store.save_feed(&next);
    hn_ok > 0
}

fn publish_gh(data: &SharedData, fresh: github::GhFetch, store: &StateStore) -> bool {
    let _write = SNAPSHOT_WRITE_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let current = data.load();
    let (gh_trending, gh_ok) = merge_keyed(fresh, &current.gh_trending, "github");

    if !gh_trending.is_empty() && gh_ok == 0 {
        tracing::error!(
            provider = "github",
            keys = gh_trending.len(),
            "every key failed"
        );
        return false;
    }

    tracing::info!(
        gh_fresh = gh_ok,
        gh_stale = gh_trending.len() - gh_ok,
        "github cache updated"
    );
    let last_fetched = next_fetched(current.last_fetched);

    let next = Arc::new(DataSnapshot {
        hn_pages: current.hn_pages.clone(),
        gh_trending,
        reddit_feed: current.reddit_feed.clone(),
        last_fetched,
    });
    data.store(next.clone());
    store.save_feed(&next);
    gh_ok > 0
}

async fn fetch_and_update_reddit(
    data: &SharedData,
    client: &Client,
    session: &mut RedditSession,
    store: &StateStore,
) -> bool {
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

    tracing::info!(
        reddit_fresh = reddit_ok,
        reddit_failed = requested - reddit_ok,
        reddit_skipped = config::REDDIT_SUBREDDITS.len() - requested,
        elapsed_secs = start.elapsed().as_secs(),
        "reddit cache updated"
    );

    if reddit_ok == 0 {
        drop(_write);
        store.save_reddit_session(session);
        return false;
    }
    let last_fetched = next_fetched(current.last_fetched);

    let next = Arc::new(DataSnapshot {
        hn_pages: current.hn_pages.clone(),
        gh_trending: current.gh_trending.clone(),
        reddit_feed,
        last_fetched,
    });
    data.store(next.clone());
    store.save_feed(&next);
    drop(_write);
    store.save_reddit_session(session);
    true
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
    fn publish_versions_are_strictly_increasing() {
        let now = Utc::now();
        let next = next_fetched(now);

        assert!(next.timestamp() > now.timestamp());
        assert!(next_fetched(next).timestamp() > next.timestamp());
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
