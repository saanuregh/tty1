use std::collections::HashMap;
use std::sync::LazyLock;

use crate::client::Client;
use scraper::{Html, Selector};
use serde::Deserialize;
use tracing::{info, warn};

use super::{FetchError, jittered_delay, text};
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

    // Sequential, jitter-paced fetches: a randomized gap between requests so the
    // cadence does not look like a fixed-interval bot. No delay before the first.
    let mut results: Vec<(&str, Vec<RedditPost>)> =
        Vec::with_capacity(config::REDDIT_SUBREDDITS.len());
    for (i, &sub) in config::REDDIT_SUBREDDITS.iter().enumerate() {
        if i > 0 {
            jittered_delay(
                config::REDDIT_REQUEST_INTERVAL_MS,
                config::REDDIT_REQUEST_JITTER_MS,
            )
            .await;
        }
        let posts = scrape_subreddit(client, sub).await.unwrap_or_else(|e| {
            warn!(subreddit = sub, error = %e, "failed to fetch subreddit");
            Vec::new()
        });
        results.push((sub, posts));
    }

    let mut feed = RedditFeed::with_capacity(results.len() + 1);

    // Merged "all" view: top N across all subreddits, sorted by score.
    let mut all: Vec<RedditPost> = results
        .iter()
        .flat_map(|(_, posts)| posts.iter().cloned())
        .collect();
    all.sort_by_key(|p| std::cmp::Reverse(p.score));
    all.truncate(config::REDDIT_ALL_VIEW_LIMIT);
    feed.insert(config::FILTER_ALL.to_string(), all);

    for (sub, posts) in results {
        feed.insert(sub.to_lowercase(), posts);
    }

    feed
}

// ===== Active source: old.reddit HTML =====
//
// Reddit disabled the public JSON API (www.reddit.com/.../hot.json now 403s),
// but old.reddit.com still serves the listing as HTML with score and comment
// counts exposed as `data-*` attributes on each `div.thing` — everything the
// JSON API used to give, in a scrapeable form.

struct RedditSelectors {
    thing: Selector,
    title: Selector,
}

static SELECTORS: LazyLock<Option<RedditSelectors>> = LazyLock::new(|| {
    Some(RedditSelectors {
        thing: Selector::parse("div.thing").ok()?,
        title: Selector::parse("a.title").ok()?,
    })
});

async fn scrape_subreddit(client: &Client, subreddit: &str) -> Result<Vec<RedditPost>, FetchError> {
    let url = format!(
        // +5 margin for stickied/promoted/deleted rows filtered out during parsing.
        "https://old.reddit.com/r/{}/hot/?limit={}",
        subreddit,
        config::REDDIT_POSTS_PER_SUB + 5
    );

    let html = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // spawn_blocking: scraper HTML parsing is CPU-bound.
    let posts = tokio::task::spawn_blocking(move || parse_listing_html(&html)).await?;
    Ok(posts)
}

fn parse_listing_html(html: &str) -> Vec<RedditPost> {
    let Some(sel) = SELECTORS.as_ref() else {
        tracing::error!("reddit: CSS selectors failed to parse");
        return Vec::new();
    };

    let document = Html::parse_document(html);

    // Detect stale markup / block pages: a valid listing always has thing rows.
    if document.select(&sel.thing).next().is_none() && html.len() > 1000 {
        tracing::error!(
            html_len = html.len(),
            "reddit: 0 things matched on non-empty page — markup changed or IP blocked"
        );
        return Vec::new();
    }

    let mut posts = Vec::with_capacity(config::REDDIT_POSTS_PER_SUB);
    for thing in document.select(&sel.thing) {
        let el = thing.value();

        // Mirror the JSON filter: drop ads, stickied announcements, and deleted authors.
        if el.attr("data-promoted") == Some("true") {
            continue;
        }
        let stickied = el
            .attr("class")
            .is_some_and(|c| c.split_whitespace().any(|x| x == "stickied"));
        if stickied {
            continue;
        }
        let author = el.attr("data-author").unwrap_or_default();
        if author.is_empty() || author == "[deleted]" {
            continue;
        }

        let Some(title) = thing
            .select(&sel.title)
            .next()
            .map(text)
            .filter(|t| !t.is_empty())
        else {
            continue;
        };

        let domain = el.attr("data-domain").unwrap_or_default().to_string();
        let is_self = domain.starts_with("self.");

        let post = RedditPost {
            title,
            url: el.attr("data-url").unwrap_or_default().to_string(),
            permalink: el.attr("data-permalink").unwrap_or_default().to_string(),
            subreddit: el.attr("data-subreddit").unwrap_or_default().to_string(),
            score: el
                .attr("data-score")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            author: author.to_string(),
            // data-timestamp is epoch milliseconds; created_at is epoch seconds.
            created_at: el
                .attr("data-timestamp")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0) as f64
                / 1000.0,
            comment_count: el
                .attr("data-comments-count")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            domain,
            is_self,
            stickied: false,
        }
        .finalize();

        posts.push(post);
        if posts.len() >= config::REDDIT_POSTS_PER_SUB {
            break;
        }
    }

    posts
}

// ===== Retained: public JSON API (disabled by Reddit) =====
//
// Reddit shut down the public .json endpoint (it now 403s), so this is unused
// and `scrape_subreddit` (old.reddit) is the live source. Kept in case Reddit
// re-enables it — same return type, so swapping it back in is a one-line change.
#[allow(dead_code)]
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
        let mut results = Vec::with_capacity(config::REDDIT_SUBREDDITS.len());
        for (i, sub) in config::REDDIT_SUBREDDITS.iter().enumerate() {
            if i > 0 {
                jittered_delay(
                    config::REDDIT_REQUEST_INTERVAL_MS,
                    config::REDDIT_REQUEST_JITTER_MS,
                )
                .await;
            }
            results.push(scrape_subreddit(&client, sub).await);
        }

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
        // The old.reddit scraper must populate real metrics, not silent zeros:
        // the top-scored post should have a positive score, and some post should
        // carry comments. Guards against stale `data-*` attribute names.
        assert!(
            all[0].score > 0,
            "top post has score 0 — data-score not parsed"
        );
        assert!(
            all.iter().any(|p| p.comment_count > 0),
            "no post has comments — data-comments-count not parsed"
        );
    }
}
