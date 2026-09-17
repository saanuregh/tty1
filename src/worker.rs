use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::cache::{DataSnapshot, HtmlSnapshot, SharedData, SharedHtml};
use crate::client::Clients;
use crate::config;
use crate::providers::{FetchError, github, hackernews, reddit};

/// Each provider fails independently — a single provider outage never blocks the others.
pub async fn run_scraper(
    data: SharedData,
    html: SharedHtml,
    clients: Clients,
    interval: Duration,
    story_count: usize,
) {
    loop {
        fetch_and_update(&data, &clients, story_count).await;
        rebuild_html(&data, &html).await;
        let jitter = Duration::from_secs(fastrand::u64(0..=config::SCRAPE_JITTER_SECS * 2));
        let sleep_dur = interval - Duration::from_secs(config::SCRAPE_JITTER_SECS) + jitter;
        tracing::debug!(sleep_secs = sleep_dur.as_secs(), "next scrape");
        tokio::time::sleep(sleep_dur).await;
    }
}

/// Separate from scraper: re-renders HTML every minute so relative timestamps ("3h ago")
/// stay fresh between the 30-minute data fetches.
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
                    error = %e,
                    retained = retained.len(),
                    "fetch failed, keeping previous"
                );
                merged.insert(key, retained);
            }
        }
    }

    (merged, fresh_count)
}

async fn fetch_and_update(data: &SharedData, clients: &Clients, story_count: usize) {
    tracing::info!("scrape cycle starting");

    let start = Instant::now();

    // All three providers fetch concurrently — each handles its own rate limiting internally.
    let (hn_fresh, gh_fresh, reddit_fresh) = tokio::join!(
        hackernews::fetch_all_pages(&clients.direct, story_count),
        github::fetch_all_trending(&clients.direct),
        reddit::fetch_reddit_feed(&clients.reddit),
    );

    let current = data.load();

    let (hn_pages, hn_ok) = merge_keyed(hn_fresh, &current.hn_pages, "hn");
    let (gh_trending, gh_ok) = merge_keyed(gh_fresh, &current.gh_trending, "github");
    let (mut reddit_feed, reddit_ok) = merge_keyed(reddit_fresh, &current.reddit_feed, "reddit");

    // Derived after the merge so the merged view draws on retained data too.
    let subreddit_count = reddit_feed.len();
    let all = reddit::build_all_view(&reddit_feed);
    reddit_feed.insert(config::FILTER_ALL.to_string(), all);

    // Per-key retention makes an outage invisible in the UI, so the log is the
    // only signal that a whole source is down.
    for (label, ok, total) in [
        ("hn", hn_ok, hn_pages.len()),
        ("github", gh_ok, gh_trending.len()),
        ("reddit", reddit_ok, subreddit_count),
    ] {
        if total > 0 && ok == 0 {
            tracing::error!(provider = label, keys = total, "every key failed");
        }
    }

    // Do not advance the clock on a total failure: the footer would claim the
    // data is fresh when nothing changed.
    let last_fetched = if hn_ok + gh_ok + reddit_ok > 0 {
        Utc::now()
    } else {
        current.last_fetched
    };

    tracing::info!(
        hn_fresh = hn_ok,
        hn_stale = hn_pages.len() - hn_ok,
        gh_fresh = gh_ok,
        gh_stale = gh_trending.len() - gh_ok,
        reddit_fresh = reddit_ok,
        reddit_stale = subreddit_count - reddit_ok,
        elapsed_secs = start.elapsed().as_secs(),
        "data cache updated"
    );

    data.store(Arc::new(DataSnapshot {
        hn_pages,
        gh_trending,
        reddit_feed,
        last_fetched,
    }));
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

    #[test]
    fn removed_key_disappears() {
        let fresh = HashMap::from([("a", Ok(vec![9]))]);
        let (merged, _) = merge_keyed(fresh, &current(), "t");
        assert!(!merged.contains_key("b"));
    }
}
