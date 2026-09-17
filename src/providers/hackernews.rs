use std::collections::HashMap;

use crate::client::Client;
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use tracing::warn;

use super::FetchError;
use crate::config;

/// Per-page story data: key is page display name ("top", "newest", "show").
pub type HnPages = HashMap<String, Vec<HnStory>>;

/// What a fetch cycle returns before it is merged with the previous snapshot.
pub type HnFetch = HashMap<String, Result<Vec<HnStory>, FetchError>>;

#[derive(Debug, Clone, Deserialize)]
pub struct HnStory {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(skip)]
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

/// Fetch all configured HN pages concurrently. Per-page failures are reported,
/// not swallowed, so the caller can retain the previous data for that page.
pub async fn fetch_all_pages(client: &Client, count: usize) -> HnFetch {
    tracing::info!(
        pages = config::HN_PAGES.len(),
        count,
        "hackernews: fetching"
    );

    let futs: Vec<_> = config::HN_PAGES
        .iter()
        .map(|&(name, endpoint)| async move {
            (name.to_string(), fetch_page(client, endpoint, count).await)
        })
        .collect();

    futures::future::join_all(futs).await.into_iter().collect()
}

/// Every item fetch failing is a source failure, not an empty page. A partial
/// result above zero is kept: some stories are better than none.
fn finalize_page(requested: usize, stories: Vec<HnStory>) -> Result<Vec<HnStory>, FetchError> {
    if requested > 0 && stories.is_empty() {
        return Err(FetchError::EmptyPage);
    }
    Ok(stories)
}

async fn fetch_page(
    client: &Client,
    endpoint: &str,
    count: usize,
) -> Result<Vec<HnStory>, FetchError> {
    let url = format!("{}/{endpoint}.json", config::HN_API_BASE);
    let ids: Vec<u64> = client.get(&url).send().await?.json().await?;
    let requested = ids.len().min(count);

    let stories: Vec<HnStory> = stream::iter(ids.into_iter().take(count))
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
        .await;

    finalize_page(requested, stories)
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
    use crate::providers::tests::test_clients;

    #[tokio::test]
    #[ignore = "live HTTP"]
    async fn fetch_all_pages_returns_stories() {
        let clients = test_clients();
        let pages = fetch_all_pages(&clients.direct, 3).await;

        assert!(!pages.is_empty(), "no pages returned");
        for (name, result) in &pages {
            let stories = result
                .as_ref()
                .unwrap_or_else(|e| panic!("{name} failed: {e}"));
            assert!(!stories.is_empty(), "{name} returned 0 stories");
            for story in stories {
                assert!(!story.title.is_empty());
                assert!(!story.hn_url.is_empty());
                assert!(!story.author.is_empty());
                assert!(story.created_at > 0);
            }
        }
    }

    #[test]
    fn total_item_failure_is_an_error() {
        assert!(matches!(
            finalize_page(30, Vec::new()),
            Err(FetchError::EmptyPage)
        ));
    }

    #[test]
    fn empty_id_list_is_not_an_error() {
        assert!(finalize_page(0, Vec::new()).unwrap().is_empty());
    }
}
