use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use crate::client::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::reddit_auth::RedditSession;
use super::{FetchError, error_chain, text};
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

/// Persisted form of one post. Mirrors `RedditPost` instead of reusing its serde
/// impls: `stickied` is `skip_serializing` for the API shape, and a field added
/// there later must not vanish from the state file silently. Fields are already
/// finalized (absolute permalink, lowercase subreddit), so restoring does not
/// call `finalize`.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PersistedRedditPost {
    title: String,
    url: String,
    permalink: String,
    subreddit: String,
    score: i64,
    author: String,
    created_at: f64,
    comment_count: u32,
    domain: String,
    is_self: bool,
    stickied: bool,
}

impl From<&RedditPost> for PersistedRedditPost {
    fn from(post: &RedditPost) -> Self {
        Self {
            title: post.title.clone(),
            url: post.url.clone(),
            permalink: post.permalink.clone(),
            subreddit: post.subreddit.clone(),
            score: post.score,
            author: post.author.clone(),
            created_at: post.created_at,
            comment_count: post.comment_count,
            domain: post.domain.clone(),
            is_self: post.is_self,
            stickied: post.stickied,
        }
    }
}

impl From<PersistedRedditPost> for RedditPost {
    fn from(state: PersistedRedditPost) -> Self {
        Self {
            title: state.title,
            url: state.url,
            permalink: state.permalink,
            subreddit: state.subreddit,
            score: state.score,
            author: state.author,
            created_at: state.created_at,
            comment_count: state.comment_count,
            domain: state.domain,
            is_self: state.is_self,
            stickied: state.stickied,
        }
    }
}

#[cfg(test)]
pub(crate) fn test_post(subreddit: &str, score: i64) -> RedditPost {
    RedditPost {
        title: "a post".into(),
        url: String::new(),
        permalink: format!("https://www.reddit.com/r/{subreddit}/comments/x"),
        subreddit: subreddit.into(),
        score,
        author: "alice".into(),
        created_at: 1_700_000_000.0,
        comment_count: 3,
        domain: String::new(),
        is_self: false,
        stickied: false,
    }
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

/// Fetch `subreddits` in order, paced like someone browsing. The merged "all"
/// view is NOT built here: it must be derived after the merge so it can draw
/// on retained data too. Subreddits not requested are absent from the result.
pub async fn fetch_reddit_feed(
    client: &Client,
    session: &mut RedditSession,
    subreddits: &[&str],
) -> RedditFetch {
    info!(subreddits = subreddits.len(), "reddit: fetching");

    let mut rng = fastrand::Rng::new();
    let mut feed = RedditFetch::with_capacity(subreddits.len());
    let mut rate_limit_wait: Option<Duration> = None;
    let mut renewed_after_401 = false;
    for (i, &sub) in subreddits.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(request_gap(&mut rng)).await;
        }
        if let Some(pause) = rate_limit_wait.take() {
            info!(
                pause_secs = pause.as_secs(),
                "reddit: rate limit nearly spent, pausing"
            );
            tokio::time::sleep(pause).await;
        }
        // One failed token request fails the rest of the cycle: asking again
        // for every subreddit would only hammer the token endpoint.
        if let Err(e) = session.ensure_token(client).await {
            tracing::error!(error = %error_chain(&e), "reddit: token request failed");
            for &rest in &subreddits[i..] {
                feed.insert(rest.to_lowercase(), Err(FetchError::TokenUnavailable));
            }
            break;
        }
        let (result, pause) = fetch_listing(client, session, sub, &mut rng).await;
        rate_limit_wait = pause;
        feed.insert(sub.to_lowercase(), result);

        // A 401 drops the token. One fresh token per cycle is worth trying;
        // a second 401 means Reddit is rejecting this client, and a new
        // token and device for every remaining subreddit would only look worse.
        if !session.has_token() {
            if renewed_after_401 {
                tracing::error!("reddit: repeated 401, stopping this cycle");
                for &rest in &subreddits[i + 1..] {
                    feed.insert(rest.to_lowercase(), Err(FetchError::TokenUnavailable));
                }
                break;
            }
            renewed_after_401 = true;
        }
    }
    feed
}

/// Subreddits to fetch this cycle, in request order: a random subset, plus
/// every configured subreddit with no posts yet, so a cold start fills every
/// panel instead of leaving some blank until a later cycle picks them.
pub fn plan_cycle(feed: &RedditFeed, rng: &mut fastrand::Rng) -> Vec<&'static str> {
    let mut cycle = pick_subset(config::REDDIT_SUBREDDITS, rng);
    for &sub in config::REDDIT_SUBREDDITS {
        let has_posts = feed
            .get(&sub.to_lowercase())
            .is_some_and(|posts| !posts.is_empty());
        if !has_posts && !cycle.contains(&sub) {
            cycle.push(sub);
        }
    }
    rng.shuffle(&mut cycle);
    cycle
}

/// Shuffled subset of `subreddits`, sized at a random percentage between
/// `REDDIT_SUBSET_MIN_PERCENT` and `REDDIT_SUBSET_MAX_PERCENT`, never empty.
fn pick_subset<'a>(subreddits: &[&'a str], rng: &mut fastrand::Rng) -> Vec<&'a str> {
    let percent = rng.u64(config::REDDIT_SUBSET_MIN_PERCENT..=config::REDDIT_SUBSET_MAX_PERCENT);
    let count = (subreddits.len() as u64 * percent).div_ceil(100).max(1) as usize;
    let mut subset = subreddits.to_vec();
    rng.shuffle(&mut subset);
    subset.truncate(count);
    subset
}

/// Gap before the next request: usually a few seconds, sometimes a long
/// pause, like someone scrolling and then stopping to read.
fn request_gap(rng: &mut fastrand::Rng) -> Duration {
    let millis = if rng.u64(0..100) < config::REDDIT_LONG_PAUSE_PERCENT {
        rng.u64(config::REDDIT_LONG_PAUSE_MIN_MS..=config::REDDIT_LONG_PAUSE_MAX_MS)
    } else {
        rng.u64(config::REDDIT_GAP_MIN_MS..=config::REDDIT_GAP_MAX_MS)
    };
    Duration::from_millis(millis)
}

/// How long to wait before the next request when the rate-limit window is
/// nearly spent: until the reset plus a little random slack, capped.
fn rate_limit_pause(
    remaining: Option<f64>,
    reset_secs: Option<u64>,
    rng: &mut fastrand::Rng,
) -> Option<Duration> {
    if remaining? > config::REDDIT_RATE_LIMIT_LOW {
        return None;
    }
    let secs = reset_secs.unwrap_or(config::REDDIT_RATE_LIMIT_MAX_PAUSE_SECS) + rng.u64(1..=5);
    Some(Duration::from_secs(
        secs.min(config::REDDIT_RATE_LIMIT_MAX_PAUSE_SECS),
    ))
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

// ===== Active source: OAuth API as the Android app =====

/// One subreddit's hot listing. Also returns how long to pause afterwards when
/// the rate-limit window is nearly spent. No retry: a failed key keeps its
/// previous data, and after a 401 the next key gets a fresh token.
async fn fetch_listing(
    client: &Client,
    session: &mut RedditSession,
    subreddit: &str,
    rng: &mut fastrand::Rng,
) -> (Result<Vec<RedditPost>, FetchError>, Option<Duration>) {
    let url = format!(
        // +5 margin for stickied/deleted posts filtered out during parsing.
        "{}/r/{}/hot?limit={}&raw_json=1",
        config::REDDIT_API_BASE,
        subreddit,
        config::REDDIT_POSTS_PER_SUB + 5
    );

    let response = match session.authorize(client.get(&url)).send().await {
        Ok(response) => response,
        Err(e) => return (Err(e.into()), None),
    };
    let pause = rate_limit_pause(
        header_value(&response, "x-ratelimit-remaining"),
        header_value(&response, "x-ratelimit-reset"),
        rng,
    );
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        session.invalidate();
    }

    let result = async {
        let body: serde_json::Value = response.error_for_status()?.json().await?;
        parse_listing_json(&body)
    }
    .await;
    (result, pause)
}

fn header_value<T: std::str::FromStr>(response: &reqwest::Response, name: &str) -> Option<T> {
    response
        .headers()
        .get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Posts from a listing body (`data.children[].data`), shared by the OAuth
/// API and the retained public JSON fetch.
fn parse_listing_json(body: &serde_json::Value) -> Result<Vec<RedditPost>, FetchError> {
    // An error body (403, rate limit) has no listing at all — not an empty one.
    let Some(children) = body["data"]["children"].as_array() else {
        return Err(FetchError::EmptyPage);
    };

    let mut posts = Vec::with_capacity(config::REDDIT_POSTS_PER_SUB);
    let mut malformed = 0;

    for child in children {
        match serde_json::from_value::<RedditPost>(child["data"].clone()) {
            Ok(post) => {
                if post.stickied || post.author == "[deleted]" {
                    continue;
                }
                posts.push(post.finalize());
                if posts.len() >= config::REDDIT_POSTS_PER_SUB {
                    break;
                }
            }
            Err(error) => {
                malformed += 1;
                tracing::warn!(%error, "reddit: dropping malformed post");
            }
        }
    }

    // A non-empty listing whose rows all fail to deserialize is schema drift,
    // not an empty subreddit. Retain the previous posts instead of blanking it.
    if !children.is_empty() && malformed == children.len() {
        return Err(FetchError::EmptyPage);
    }

    Ok(posts)
}

// ===== Retained: old.reddit HTML (now behind a login) =====

//
// old.reddit.com serves the listing as HTML with score and comment counts
// exposed as `data-*` attributes on each `div.thing`. It now redirects
// anonymous requests to a login page, so this is unused. Kept in case that
// changes; like `fetch_subreddit`, it needs a client with browser headers.

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

#[allow(dead_code)]
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
// and `fetch_listing` (OAuth API) is the live source. Kept in case Reddit
// re-enables it. Reviving it needs a client with browser headers: the Reddit
// client now sends only the Android headers added per request.
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

    parse_listing_json(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::REDDIT_SUBREDDITS, providers::tests::test_clients};

    #[tokio::test]
    #[ignore = "live HTTP"]
    async fn fetch_listing_live() {
        if !crate::config::reddit_scraper_enabled() {
            return;
        }
        let clients = test_clients();
        let mut session = RedditSession::new();
        let mut rng = fastrand::Rng::new();
        session.ensure_token(&clients.reddit).await.unwrap();
        let (result, _) = fetch_listing(
            &clients.reddit,
            &mut session,
            REDDIT_SUBREDDITS[0],
            &mut rng,
        )
        .await;
        assert!(!result.unwrap().is_empty(), "listing returned no posts");
    }

    fn listing(children: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "kind": "Listing", "data": { "children": children } })
    }

    fn child(title: &str, author: &str, stickied: bool) -> serde_json::Value {
        serde_json::json!({ "kind": "t3", "data": {
            "title": title,
            "url": "https://example.com/x",
            "permalink": "/r/rust/comments/abc/x/",
            "subreddit": "Rust",
            "score": 7,
            "author": author,
            "created_utc": 1_700_000_000.0,
            "num_comments": 3,
            "domain": "example.com",
            "is_self": false,
            "stickied": stickied,
        }})
    }

    #[test]
    fn parse_listing_filters_stickied_and_deleted() {
        let body = listing(serde_json::json!([
            child("keep", "alice", false),
            child("pinned", "mod", true),
            child("gone", "[deleted]", false),
        ]));
        let posts = parse_listing_json(&body).unwrap();
        let titles: Vec<&str> = posts.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, vec!["keep"]);
    }

    #[test]
    fn parse_listing_without_children_is_empty_page() {
        let body = serde_json::json!({ "message": "Forbidden", "error": 403 });
        assert!(matches!(
            parse_listing_json(&body),
            Err(FetchError::EmptyPage)
        ));
    }

    #[test]
    fn all_malformed_children_is_empty_page() {
        let body = listing(serde_json::json!([
            { "kind": "t3", "data": { "score": null } },
            { "kind": "t3", "data": { "title": 42 } },
        ]));
        assert!(matches!(
            parse_listing_json(&body),
            Err(FetchError::EmptyPage)
        ));
    }

    #[test]
    fn all_filtered_children_is_an_empty_ok() {
        let body = listing(serde_json::json!([
            child("pinned", "mod", true),
            child("gone", "[deleted]", false),
        ]));
        assert!(parse_listing_json(&body).unwrap().is_empty());
    }

    #[test]
    fn parse_listing_finalizes_permalink() {
        let body = listing(serde_json::json!([child("keep", "alice", false)]));
        let post = &parse_listing_json(&body).unwrap()[0];
        assert_eq!(
            post.permalink,
            "https://www.reddit.com/r/rust/comments/abc/x/"
        );
        assert_eq!(post.subreddit, "rust");
    }

    #[test]
    fn subset_size_within_bounds() {
        let subs: Vec<&str> = (0..50).map(|_| "s").collect();
        let mut rng = fastrand::Rng::with_seed(1);
        for _ in 0..200 {
            let n = pick_subset(&subs, &mut rng).len() as u64;
            assert!(
                n >= 50 * config::REDDIT_SUBSET_MIN_PERCENT / 100,
                "{n} too small"
            );
            assert!(
                n <= 50 * config::REDDIT_SUBSET_MAX_PERCENT / 100,
                "{n} too large"
            );
        }
        assert_eq!(pick_subset(&["only"], &mut rng), vec!["only"]);
    }

    #[test]
    fn subset_is_unique_and_configured() {
        let mut rng = fastrand::Rng::with_seed(2);
        let subset = pick_subset(REDDIT_SUBREDDITS, &mut rng);
        let unique: std::collections::HashSet<_> = subset.iter().collect();
        assert_eq!(unique.len(), subset.len());
        assert!(subset.iter().all(|s| REDDIT_SUBREDDITS.contains(s)));
    }

    #[test]
    fn plan_cycle_includes_every_subreddit_without_posts() {
        let mut rng = fastrand::Rng::with_seed(5);
        let all = plan_cycle(&RedditFeed::new(), &mut rng);
        assert_eq!(
            all.len(),
            REDDIT_SUBREDDITS.len(),
            "cold start fetches everything"
        );

        let full: RedditFeed = REDDIT_SUBREDDITS
            .iter()
            .map(|s| (s.to_lowercase(), vec![post(s, 1)]))
            .collect();
        let warm = plan_cycle(&full, &mut rng);
        assert!(
            warm.len() < REDDIT_SUBREDDITS.len(),
            "warm cycle fetches a subset"
        );
    }

    #[test]
    fn request_gap_within_bounds() {
        let mut rng = fastrand::Rng::with_seed(3);
        let (mut short, mut long) = (0, 0);
        for _ in 0..1000 {
            let ms = request_gap(&mut rng).as_millis() as u64;
            if (config::REDDIT_GAP_MIN_MS..=config::REDDIT_GAP_MAX_MS).contains(&ms) {
                short += 1;
            } else if (config::REDDIT_LONG_PAUSE_MIN_MS..=config::REDDIT_LONG_PAUSE_MAX_MS)
                .contains(&ms)
            {
                long += 1;
            } else {
                panic!("gap {ms} ms outside both ranges");
            }
        }
        assert!(short > 0 && long > 0, "short={short} long={long}");
    }

    #[test]
    fn rate_limit_pause_only_when_low() {
        let mut rng = fastrand::Rng::with_seed(4);
        assert_eq!(rate_limit_pause(Some(50.0), Some(300), &mut rng), None);
        assert_eq!(rate_limit_pause(None, Some(300), &mut rng), None);

        let pause = rate_limit_pause(Some(3.0), Some(300), &mut rng).unwrap();
        assert!((301..=305).contains(&pause.as_secs()), "{pause:?}");

        let capped = rate_limit_pause(Some(0.0), Some(99_999), &mut rng).unwrap();
        assert_eq!(capped.as_secs(), config::REDDIT_RATE_LIMIT_MAX_PAUSE_SECS);
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
    fn reddit_post_state_round_trips_all_fields() {
        let post = RedditPost {
            title: "a post".into(),
            url: "https://example.com/x".into(),
            permalink: "https://www.reddit.com/r/rust/comments/abc/x".into(),
            subreddit: "rust".into(),
            score: 42,
            author: "alice".into(),
            created_at: 1_700_000_000.5,
            comment_count: 7,
            domain: "example.com".into(),
            is_self: false,
            stickied: true,
        };
        let restored = RedditPost::from(PersistedRedditPost::from(&post));

        assert_eq!(restored.title, post.title);
        assert_eq!(restored.url, post.url);
        assert_eq!(restored.permalink, post.permalink);
        assert_eq!(restored.subreddit, post.subreddit);
        assert_eq!(restored.score, post.score);
        assert_eq!(restored.author, post.author);
        assert_eq!(restored.created_at, post.created_at);
        assert_eq!(restored.comment_count, post.comment_count);
        assert_eq!(restored.domain, post.domain);
        assert_eq!(restored.is_self, post.is_self);
        assert_eq!(restored.stickied, post.stickied);
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
