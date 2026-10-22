use std::collections::HashMap;

use crate::client::Client;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
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

/// Persisted form of one story. Separate from `HnStory`, whose API deserializer
/// skips the derived `hn_url` and `domain` and whose `id` is private. Building it
/// field by field turns a newly added field into a compile error instead of a
/// silent loss on restore.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PersistedHnStory {
    id: u64,
    title: String,
    url: Option<String>,
    hn_url: String,
    score: u32,
    author: String,
    created_at: u64,
    comment_count: u32,
    domain: Option<String>,
}

impl From<&HnStory> for PersistedHnStory {
    fn from(story: &HnStory) -> Self {
        Self {
            id: story.id,
            title: story.title.clone(),
            url: story.url.clone(),
            hn_url: story.hn_url.clone(),
            score: story.score,
            author: story.author.clone(),
            created_at: story.created_at,
            comment_count: story.comment_count,
            domain: story.domain.clone(),
        }
    }
}

/// Restores the derived fields verbatim: the derivation in `fetch_item` runs
/// only on fresh API data, so it must not run again here.
impl From<PersistedHnStory> for HnStory {
    fn from(state: PersistedHnStory) -> Self {
        Self {
            id: state.id,
            title: state.title,
            url: state.url,
            hn_url: state.hn_url,
            score: state.score,
            author: state.author,
            created_at: state.created_at,
            comment_count: state.comment_count,
            domain: state.domain,
        }
    }
}

#[cfg(test)]
pub(crate) fn test_story(title: &str) -> HnStory {
    HnStory {
        id: 1,
        title: title.into(),
        url: Some("https://example.com/a".into()),
        hn_url: "https://news.ycombinator.com/item?id=1".into(),
        score: 10,
        author: "alice".into(),
        created_at: 1_700_000_000,
        comment_count: 2,
        domain: Some("example.com".into()),
    }
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

/// An empty HN page is a source failure, not a legitimate empty listing. An
/// empty index response would otherwise overwrite the previous page and blank
/// the panel. A partial result above zero is kept: some stories are better than
/// none.
fn finalize_page(stories: Vec<HnStory>) -> Result<Vec<HnStory>, FetchError> {
    if stories.is_empty() {
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
    if ids.is_empty() {
        warn!(endpoint, "hn index returned no ids");
        return Err(FetchError::EmptyPage);
    }

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

    finalize_page(stories)
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

    fn story() -> HnStory {
        HnStory {
            id: 42,
            title: "Show HN: a thing".into(),
            url: Some("https://example.com/thing".into()),
            hn_url: "https://news.ycombinator.com/item?id=42".into(),
            score: 123,
            author: "alice".into(),
            created_at: 1_700_000_000,
            comment_count: 45,
            domain: Some("example.com".into()),
        }
    }

    #[test]
    fn hn_story_state_keeps_derived_fields() {
        let story = story();
        let restored = HnStory::from(PersistedHnStory::from(&story));

        assert_eq!(restored.id, story.id);
        assert_eq!(restored.title, story.title);
        assert_eq!(restored.url, story.url);
        assert_eq!(restored.hn_url, story.hn_url);
        assert_eq!(restored.score, story.score);
        assert_eq!(restored.author, story.author);
        assert_eq!(restored.created_at, story.created_at);
        assert_eq!(restored.comment_count, story.comment_count);
        assert_eq!(restored.domain, story.domain);
    }

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
    fn empty_page_is_an_error() {
        assert!(matches!(
            finalize_page(Vec::new()),
            Err(FetchError::EmptyPage)
        ));
    }
}
