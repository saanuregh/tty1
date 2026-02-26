use std::collections::HashMap;

use crate::client::Client;
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use tracing::{info, warn};

use super::FetchError;
use crate::config;

/// Key: subreddit name (lowercase) or "all" for the merged top-N view.
pub type RedditFeed = HashMap<String, Vec<RedditPost>>;

#[derive(Debug, Clone, Deserialize)]
pub struct RedditPost {
    pub title: String,
    #[serde(default)]
    pub url: String,
    pub permalink: String,
    pub subreddit: String,
    pub score: i64,
    #[serde(default)]
    pub author: String,
    #[serde(rename = "created_utc")]
    pub created_at: f64,
    #[serde(rename = "num_comments")]
    pub comment_count: u32,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub is_self: bool,
    #[serde(default, skip_serializing)]
    pub stickied: bool,
}

impl RedditPost {
    fn finalize(mut self) -> Self {
        self.permalink = format!("https://www.reddit.com{}", self.permalink);
        if self.is_self {
            self.url = self.permalink.clone();
        }
        self.subreddit = self.subreddit.to_lowercase();
        self
    }
}

/// Returns a feed keyed by subreddit name + an "all" entry with the merged top-N.
pub async fn fetch_reddit_feed(client: &Client) -> RedditFeed {
    info!(
        subreddits = config::REDDIT_SUBREDDITS.len(),
        "reddit: fetching"
    );

    let futs: Vec<_> = config::REDDIT_SUBREDDITS
        .iter()
        .map(|&sub| async move {
            let posts = fetch_subreddit(client, sub).await.unwrap_or_else(|e| {
                warn!(subreddit = sub, error = %e, "failed to fetch subreddit");
                Vec::new()
            });
            (sub, posts)
        })
        .collect();

    let results: Vec<_> = stream::iter(futs)
        .buffered(config::REDDIT_CONCURRENT_FETCHES)
        .collect()
        .await;

    let mut feed = RedditFeed::with_capacity(results.len() + 1);

    // Merged "all" view: top N across all subreddits, sorted by score.
    let mut all: Vec<RedditPost> = results
        .iter()
        .flat_map(|(_, posts)| posts.iter().cloned())
        .collect();
    all.sort_by(|a, b| b.score.cmp(&a.score));
    all.truncate(config::REDDIT_ALL_VIEW_LIMIT);
    feed.insert(config::FILTER_ALL.to_string(), all);

    for (sub, posts) in results {
        feed.insert(sub.to_lowercase(), posts);
    }

    feed
}

async fn fetch_subreddit(client: &Client, subreddit: &str) -> Result<Vec<RedditPost>, FetchError> {
    let url = format!(
        // Public .json endpoint avoids OAuth complexity. +5 margin for stickied/deleted posts filtered below.
        "https://www.reddit.com/r/{}/hot.json?limit={}&raw_json=1",
        subreddit,
        config::REDDIT_POSTS_PER_SUB + 5
    );

    let body: serde_json::Value = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let empty = vec![];
    let children = body["data"]["children"].as_array().unwrap_or(&empty);

    let posts: Vec<_> = children
        .iter()
        .filter_map(|c| serde_json::from_value::<RedditPost>(c["data"].clone()).ok())
        .filter(|p| !p.stickied && p.author != "[deleted]")
        .take(config::REDDIT_POSTS_PER_SUB)
        .map(RedditPost::finalize)
        .collect();

    Ok(posts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::tests::test_client;

    #[tokio::test]
    async fn all_subreddits_reachable() {
        let client = test_client();
        let futs: Vec<_> = config::REDDIT_SUBREDDITS
            .iter()
            .map(|sub| fetch_subreddit(&client, sub))
            .collect();
        let results: Vec<_> = stream::iter(futs)
            .buffered(config::REDDIT_CONCURRENT_FETCHES)
            .collect()
            .await;

        let failures: Vec<_> = config::REDDIT_SUBREDDITS
            .iter()
            .zip(results)
            .filter_map(|(sub, result)| match result {
                Ok(posts) if posts.is_empty() => Some(format!("r/{sub}: 0 posts")),
                Err(e) => Some(format!("r/{sub}: {e}")),
                _ => None,
            })
            .collect();

        assert!(
            failures.is_empty(),
            "Failed subreddits:\n{}",
            failures.join("\n")
        );
    }

    #[tokio::test]
    async fn feed_returns_keyed_sorted_posts() {
        let client = test_client();
        let feed = fetch_reddit_feed(&client).await;

        let all = feed.get("all").expect("missing 'all' key");
        assert!(!all.is_empty(), "all feed returned no posts");
        assert!(all.len() <= config::REDDIT_ALL_VIEW_LIMIT);
        for post in all {
            assert!(!post.title.is_empty());
            assert!(post.permalink.starts_with("https://www.reddit.com/"));
        }
        for w in all.windows(2) {
            assert!(
                w[0].score >= w[1].score,
                "not sorted: {} < {}",
                w[0].score,
                w[1].score
            );
        }
    }
}
