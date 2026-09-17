use std::collections::HashMap;
use std::sync::LazyLock;

use crate::client::Client;
use scraper::{Html, Selector};
use serde::Deserialize;
use tracing::info;

use super::{FetchError, jittered_delay, text};
use crate::config;

/// Key: subreddit name (lowercase) or "all" for the merged top-N view.
pub type RedditFeed = HashMap<String, Vec<RedditPost>>;

/// What a fetch cycle returns before it is merged with the previous snapshot.
/// Holds subreddit keys only — the "all" view is derived after the merge.
pub type RedditFetch = HashMap<String, Result<Vec<RedditPost>, FetchError>>;

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

/// Fetch every configured subreddit. The merged "all" view is NOT built here:
/// it must be derived after the merge so it can draw on retained data too.
pub async fn fetch_reddit_feed(client: &Client) -> RedditFetch {
    info!(
        subreddits = config::REDDIT_SUBREDDITS.len(),
        "reddit: fetching"
    );

    // Sequential, jitter-paced fetches: a randomized gap between requests so the
    // cadence does not look like a fixed-interval bot. No delay before the first.
    let mut feed = RedditFetch::with_capacity(config::REDDIT_SUBREDDITS.len());
    for (i, &sub) in config::REDDIT_SUBREDDITS.iter().enumerate() {
        if i > 0 {
            jittered_delay(
                config::REDDIT_REQUEST_INTERVAL_MS,
                config::REDDIT_REQUEST_JITTER_MS,
            )
            .await;
        }
        feed.insert(sub.to_lowercase(), fetch_subreddit(client, sub).await);
    }
    feed
}

/// Top N posts across every subreddit, by score. Skips an existing "all" key so
/// it is safe to call on a map that already holds one.
pub fn build_all_view(feed: &RedditFeed) -> Vec<RedditPost> {
    let mut all: Vec<RedditPost> = feed
        .iter()
        .filter(|(key, _)| key.as_str() != config::FILTER_ALL)
        .flat_map(|(_, posts)| posts.iter().cloned())
        .collect();
    all.sort_by_key(|p| std::cmp::Reverse(p.score));
    all.truncate(config::REDDIT_ALL_VIEW_LIMIT);
    all
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
    let posts = tokio::task::spawn_blocking(move || parse_listing_html(&html)).await??;
    Ok(posts)
}

fn parse_listing_html(html: &str) -> Result<Vec<RedditPost>, FetchError> {
    let Some(sel) = SELECTORS.as_ref() else {
        tracing::error!("reddit: CSS selectors failed to parse");
        return Err(FetchError::EmptyPage);
    };

    let document = Html::parse_document(html);

    dbg!(&document);

    // Detect stale markup / block pages: a valid listing always has thing rows.
    if document.select(&sel.thing).next().is_none() && html.len() > 1000 {
        tracing::error!(
            html_len = html.len(),
            "reddit: 0 things matched on non-empty page — markup changed or IP blocked"
        );
        return Err(FetchError::EmptyPage);
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

    Ok(posts)
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
    use crate::{config::REDDIT_SUBREDDITS, providers::tests::test_clients};


    #[tokio::test]
    async fn test_fetch_subreddit() {
        let clients = test_clients();
        let result = fetch_subreddit(&clients.reddit, REDDIT_SUBREDDITS[0])
            .await
            .unwrap();
        assert!(!result.is_empty(), "all feed returned no posts");
    }

    #[tokio::test]
    async fn test_scrape_subreddit() {
        let clients = test_clients();
        let result = scrape_subreddit(&clients.reddit, REDDIT_SUBREDDITS[0])
            .await
            .unwrap();
        assert!(!result.is_empty(), "all feed returned no posts");
    }

    fn post(subreddit: &str, score: i64) -> RedditPost {
        RedditPost {
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
        }
    }

    #[test]
    fn all_view_sorts_by_score_and_skips_existing_all() {
        let feed = RedditFeed::from([
            ("rust".to_string(), vec![post("rust", 5), post("rust", 50)]),
            ("golang".to_string(), vec![post("golang", 20)]),
            ("all".to_string(), vec![post("stale", 9999)]),
        ]);

        let all = build_all_view(&feed);
        let scores: Vec<i64> = all.iter().map(|p| p.score).collect();
        assert_eq!(scores, vec![50, 20, 5]);
    }

    #[test]
    fn all_view_truncates_to_the_limit() {
        let posts: Vec<RedditPost> = (0..config::REDDIT_ALL_VIEW_LIMIT + 10)
            .map(|i| post("rust", i as i64))
            .collect();
        let feed = RedditFeed::from([("rust".to_string(), posts)]);
        assert_eq!(build_all_view(&feed).len(), config::REDDIT_ALL_VIEW_LIMIT);
    }

    #[test]
    fn blocked_page_is_an_error() {
        let html = format!("<html><body>{}</body></html>", "x".repeat(1200));
        assert!(matches!(
            parse_listing_html(&html),
            Err(FetchError::EmptyPage)
        ));
    }

    #[test]
    fn short_empty_page_is_not_an_error() {
        let posts = parse_listing_html("<html><body></body></html>").unwrap();
        assert!(posts.is_empty());
    }
}
