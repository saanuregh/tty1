use std::collections::HashMap;

use crate::client::Client;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::FetchError;
use crate::config;

/// Per-page story data: key is page display name ("top", "newest", "show").
pub type HnPages = HashMap<String, Vec<HnStory>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnStory {
    #[serde(default, skip_serializing)]
    id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, skip_deserializing)]
    pub hn_url: String,
    #[serde(default)]
    pub score: u32,
    #[serde(default, rename(deserialize = "by"))]
    pub author: String,
    #[serde(default, rename(deserialize = "time"))]
    pub created_at: u64,
    #[serde(
        default,
        rename(deserialize = "descendants"),
        deserialize_with = "deserialize_comment_count"
    )]
    pub comment_count: u32,
    #[serde(default, skip_deserializing)]
    pub domain: Option<String>,
}

fn deserialize_comment_count<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    Ok(Option::<u32>::deserialize(d)?.unwrap_or(0))
}

/// Fetch all configured HN pages concurrently, returning a map of page name → stories.
/// Individual page/story failures are logged and skipped.
pub async fn fetch_all_pages(client: &Client, count: usize) -> HnPages {
    tracing::info!(
        pages = config::HN_PAGES.len(),
        count,
        "hackernews: fetching"
    );

    let futs: Vec<_> = config::HN_PAGES
        .iter()
        .map(|&(name, endpoint)| async move {
            let stories = fetch_page(client, endpoint, count)
                .await
                .unwrap_or_else(|e| {
                    warn!(page = name, error = %e, "failed to fetch HN page");
                    Vec::new()
                });
            (name.to_string(), stories)
        })
        .collect();

    futures::future::join_all(futs).await.into_iter().collect()
}

async fn fetch_page(
    client: &Client,
    endpoint: &str,
    count: usize,
) -> Result<Vec<HnStory>, FetchError> {
    let url = format!("{}/{endpoint}.json", config::HN_API_BASE);
    let ids: Vec<u64> = client.get(&url).send().await?.json().await?;

    Ok(stream::iter(ids.into_iter().take(count))
        .map(|id| {
            let client = client.clone();
            async move {
                let url = format!("{}/item/{id}.json", config::HN_API_BASE);
                fetch_item(&client, &url)
                    .await
                    .map_err(|e| warn!("hn item {id}: {e}"))
                    .ok()
            }
        })
        .buffered(config::HN_CONCURRENT_FETCHES)
        .filter_map(|s| async { s })
        .collect()
        .await)
}

async fn fetch_item(client: &Client, url: &str) -> Result<HnStory, FetchError> {
    let mut story: HnStory = client.get(url).send().await?.json().await?;
    story.hn_url = format!("https://news.ycombinator.com/item?id={}", story.id);
    story.domain = story
        .url
        .as_deref()
        .and_then(extract_domain)
        .map(String::from);
    Ok(story)
}

/// Extract the domain from a URL, stripping any "www." prefix.
fn extract_domain(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://")?.1;
    let host = after_scheme
        .split_once('/')
        .map_or(after_scheme, |(h, _)| h);
    let host = host.split_once(':').map_or(host, |(h, _)| h);
    Some(host.strip_prefix("www.").unwrap_or(host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::tests::test_client;

    #[tokio::test]
    async fn fetch_all_pages_returns_stories() {
        let client = test_client();
        let pages = fetch_all_pages(&client, 3).await;

        assert!(!pages.is_empty(), "no pages returned");
        for (name, stories) in &pages {
            assert!(!stories.is_empty(), "{name} returned 0 stories");
            for story in stories {
                assert!(!story.title.is_empty());
                assert!(!story.hn_url.is_empty());
                assert!(!story.author.is_empty());
                assert!(story.created_at > 0);
            }
        }
    }
}
