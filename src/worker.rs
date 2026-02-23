use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::client::Client;

use crate::cache::{DataSnapshot, HtmlSnapshot, SharedData, SharedHtml};
use crate::providers::{github, hackernews, reddit};

/// Each provider fails independently — a single provider outage never blocks the others.
pub async fn run_scraper(
    data: SharedData,
    html: SharedHtml,
    client: Client,
    interval: Duration,
    story_count: usize,
) {
    loop {
        let (hn, gh, reddit, elapsed) = fetch_and_update(&data, &client, story_count).await;
        tracing::info!(
            hn,
            gh,
            reddit,
            elapsed_secs = elapsed.as_secs(),
            "data cache updated"
        );
        rebuild_html(&data, &html).await;
        tokio::time::sleep(interval).await;
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

/// Keep `current` when all vecs in `new_data` are empty but `current` has data.
/// A map with keys but empty vecs means the fetch returned structure but no actual data.
fn keep_if_empty<K, V>(
    new_data: std::collections::HashMap<K, Vec<V>>,
    current: &std::collections::HashMap<K, Vec<V>>,
    label: &str,
) -> std::collections::HashMap<K, Vec<V>>
where
    K: std::hash::Hash + Eq + Clone,
    V: Clone,
{
    if new_data.values().all(|v| v.is_empty()) && !current.values().all(|v| v.is_empty()) {
        tracing::warn!("{label} returned empty data, keeping previous");
        current.clone()
    } else {
        new_data
    }
}

async fn fetch_and_update(
    data: &SharedData,
    client: &Client,
    story_count: usize,
) -> (usize, usize, usize, Duration) {
    tracing::info!("scrape cycle starting");

    let start = Instant::now();

    // All three providers fetch concurrently — each handles its own rate limiting internally.
    let (hn_pages, gh_trending, reddit_feed) = tokio::join!(
        hackernews::fetch_all_pages(client, story_count),
        github::fetch_all_trending(client),
        reddit::fetch_reddit_feed(client),
    );

    let current = data.load();

    let hn_pages = keep_if_empty(hn_pages, &current.hn_pages, "HN");
    let gh_trending = keep_if_empty(gh_trending, &current.gh_trending, "GitHub trending");
    let reddit_feed = keep_if_empty(reddit_feed, &current.reddit_feed, "Reddit");

    let hn_count: usize = hn_pages.values().map(|v| v.len()).sum();
    let gh_count: usize = gh_trending.values().map(|v| v.len()).sum();
    let reddit_count: usize = reddit_feed.values().map(|v| v.len()).sum();

    let last_fetched = Utc::now();
    let etag = crate::cache::compute_etag(last_fetched.timestamp_millis().to_string().as_bytes());

    data.store(Arc::new(DataSnapshot {
        hn_pages,
        gh_trending,
        reddit_feed,
        last_fetched,
        etag,
    }));

    (hn_count, gh_count, reddit_count, start.elapsed())
}
