use chrono::{DateTime, Utc};
use maud::{Markup, PreEscaped, html};

use super::shell::page_shell;
use super::utils::{SEP, fmt_num, format_time_ago};
use crate::config;
use crate::providers::github::{GhTrending, TrendingRepo};
use crate::providers::hackernews::{HnPages, HnStory};
use crate::providers::reddit::{RedditFeed, RedditPost};

/// The first-paint selection. Values match the select/radio values in the
/// dashboard markup: HN page name, GH period, lowercased GH language, and the
/// configured Reddit subreddit name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub hn: String,
    pub period: String,
    pub lang: String,
    pub sub: String,
}

impl Default for View {
    fn default() -> Self {
        Self {
            hn: config::HN_PAGES[0].0.to_string(),
            period: config::GITHUB_PERIODS[0].to_string(),
            lang: config::FILTER_ALL.to_string(),
            sub: config::FILTER_ALL.to_string(),
        }
    }
}

impl View {
    /// Falls back per field when a query or cookie carries an unknown value.
    pub fn normalized(self) -> Self {
        let default = Self::default();
        Self {
            hn: if valid_hn(&self.hn) {
                self.hn
            } else {
                default.hn
            },
            period: if valid_period(&self.period) {
                self.period
            } else {
                default.period
            },
            lang: if valid_lang(&self.lang) {
                self.lang
            } else {
                default.lang
            },
            sub: if valid_sub(&self.sub) {
                self.sub
            } else {
                default.sub
            },
        }
    }
}

pub fn valid_hn(value: &str) -> bool {
    config::HN_PAGES.iter().any(|&(name, _)| name == value)
}

pub fn valid_period(value: &str) -> bool {
    config::GITHUB_PERIODS.contains(&value)
}

pub fn valid_lang(value: &str) -> bool {
    lang_display(value).is_some()
}

pub fn valid_sub(value: &str) -> bool {
    value == config::FILTER_ALL || config::REDDIT_SUBREDDITS.contains(&value)
}

fn safe_http_url(url: &str) -> Option<&str> {
    let url = url.trim();
    let scheme = url.split_once(':')?.0;
    (matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
        && !url.chars().any(char::is_control))
    .then_some(url)
}

fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    matches!(bytes.len(), 4 | 7)
        && bytes.first() == Some(&b'#')
        && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
}

/// GitHub trending keys use the display name (`Rust`); the DOM uses lowercase
/// (`rust`). `all` is its own key in both.
pub fn lang_display(value: &str) -> Option<&'static str> {
    if value == config::FILTER_ALL {
        return Some(config::FILTER_ALL);
    }
    config::GITHUB_LANGUAGES
        .iter()
        .find(|&&(name, _)| name.to_lowercase() == value)
        .map(|&(name, _)| name)
}

pub fn render_page(
    hn_pages: &HnPages,
    gh_trending: &GhTrending,
    reddit_feed: &RedditFeed,
    last_fetched: DateTime<Utc>,
    view: &View,
) -> String {
    let view = view.clone().normalized();
    let version = last_fetched.timestamp() as u64;

    page_shell(super::DASHBOARD_CSS, html! {
        main #main-content .dashboard {
            h1.visually-hidden { "tty1 tech news dashboard" }
            section.panel.hn-panel aria-label="Hacker News" {
                (panel_header(
                    include_str!("../static/icons/hn.svg"),
                    "",
                    "hn-link",
                    "Hackernews",
                    "https://news.ycombinator.com/",
                    html! {
                        select.hn-select aria-label="HN page" {
                            @for &(name, _) in config::HN_PAGES {
                                option value=(name) selected[name == view.hn] { (name) }
                            }
                        }
                    },
                ))
                (render_hn_pages(hn_pages, &view, version))
            }
            section.panel.gh-panel aria-label="GitHub Trending" {
                (panel_header(
                    include_str!("../static/icons/github.svg"),
                    "gh-icon",
                    "gh-link",
                    "GitHub Trending",
                    "https://github.com/trending",
                    html! {
                        .tab-labels role="group" aria-label="Trending period" {
                            @for &period in config::GITHUB_PERIODS {
                                input id=(format!("gh-tab-{period}"))
                                      type="radio" name="gh-tab"
                                      checked[period == view.period];
                                label .active[period == view.period]
                                      for=(format!("gh-tab-{period}"))
                                      { (&period[..1]) }
                            }
                        }
                        select.lang-select aria-label="Programming language" {
                            option value=(config::FILTER_ALL) selected[view.lang == config::FILTER_ALL] { (config::FILTER_ALL) }
                            @for &(name, _) in config::GITHUB_LANGUAGES {
                                option value=(name.to_lowercase()) selected[name.to_lowercase() == view.lang] { (name) }
                            }
                        }
                    },
                ))
                .gh-tabs {
                    @for &period in config::GITHUB_PERIODS {
                        .tab-content .active[period == view.period]
                            id=(format!("gh-{period}"))
                        {
                            (render_gh_trending(gh_trending, period, &view, version))
                        }
                    }
                }
            }
            section.panel.reddit-panel aria-label="Reddit" {
                (panel_header(
                    include_str!("../static/icons/reddit.svg"),
                    "reddit-icon",
                    "reddit-link",
                    "Reddit",
                    "https://www.reddit.com",
                    html! {
                        select.subreddit-select aria-label="Subreddit" {
                            option value=(config::FILTER_ALL) selected[view.sub == config::FILTER_ALL] { (config::FILTER_ALL) }
                            @for sub in config::REDDIT_SUBREDDITS {
                                option value=(sub) selected[*sub == view.sub] { "r/" (sub) }
                            }
                        }
                    },
                ))
                (render_reddit_feed(reddit_feed, &view, version))
            }
        }
        nav.swipe-dots aria-label="Panel navigation" {
            button.swipe-dot.dot-hn.active type="button" aria-label="Hacker News" {}
            button.swipe-dot.dot-gh type="button" aria-label="GitHub" {}
            button.swipe-dot.dot-reddit type="button" aria-label="Reddit" {}
        }
        @let fetched_ts = last_fetched.timestamp() as u64;
        footer.last-updated aria-live="polite" {
            "updated "
            time.last-updated-time data-ts=(fetched_ts) { (format_time_ago(fetched_ts)) }
            span.sep { (SEP) }
            a.settings-link href="/settings" { "settings" }
        }
        script { (PreEscaped(super::DASHBOARD_JS)) }
    }).into_string()
}

fn panel_header(
    icon: &'static str,
    icon_class: &'static str,
    link_class: &'static str,
    name: &'static str,
    href: &'static str,
    controls: Markup,
) -> Markup {
    let icon_class = if icon_class.is_empty() {
        "source-icon".to_string()
    } else {
        format!("source-icon {icon_class}")
    };
    let link_class = format!("source-name {link_class}");
    html! {
        header.panel-header {
            span class=(icon_class) aria-hidden="true" { (PreEscaped(icon)) }
            a class=(link_class) href=(href) target="_blank" rel="noopener" { (name) }
            (controls)
        }
    }
}

// Item renderers

fn render_hn_story(story: &HnStory) -> Markup {
    let hn_url = safe_http_url(&story.hn_url).unwrap_or("#");
    let link_url = story
        .url
        .as_deref()
        .and_then(safe_http_url)
        .or_else(|| safe_http_url(&story.hn_url))
        .unwrap_or("#");

    html! {
        span.story-title {
            a href=(link_url) {
                (story.title)
            }
            @if let Some(domain) = &story.domain {
                span.story-domain { "(" (domain) ")" }
            }
        }
        div.story-meta {
            span.dot {}
            span.points { (story.score) " pts" }
            span.sep { (SEP) }
            (story.author)
            span.sep { (SEP) }
            time.time-ago data-ts=(story.created_at) { (format_time_ago(story.created_at)) }
            span.sep { (SEP) }
            a href=(hn_url) {
                (story.comment_count) " comments"
            }
        }
    }
}

fn render_gh_repo(repo: &TrendingRepo) -> Markup {
    html! {
        span.repo-title {
            a href=(repo.url) {
                span.repo-author { (&repo.author) "/" }
                (&repo.name)
            }
        }
        @if !repo.description.is_empty() {
            details.repo-desc {
                summary { (&repo.description) }
            }
        }
        div.repo-meta {
            @if let Some(lang) = &repo.language {
                @if let Some(color) = repo
                    .language_color
                    .as_deref()
                    .filter(|color| is_hex_color(color))
                {
                    svg.lang-dot viewBox="0 0 8 8" aria-hidden="true" {
                        circle cx="4" cy="4" r="4" fill=(color) {}
                    }
                }
                span.repo-lang { (lang) }
                span.sep { (SEP) }
            }
            span.repo-stars { "\u{2605} " (fmt_num(repo.stars)) }
            span.sep { (SEP) }
            span.repo-forks { (fmt_num(repo.forks)) " forks" }
            @if !repo.period_stars.is_empty() {
                span.sep { (SEP) }
                span.period-stars { (&repo.period_stars) }
            }
        }
    }
}

fn render_reddit_post(post: &RedditPost) -> Markup {
    let created_ts = post.created_at as u64;
    let permalink = safe_http_url(&post.permalink).unwrap_or("#");
    let link_url = safe_http_url(&post.url)
        .or_else(|| safe_http_url(&post.permalink))
        .unwrap_or("#");
    html! {
        span.reddit-post-title {
            a href=(link_url) {
                (post.title)
            }
            span.reddit-sub { "r/" (post.subreddit) }
        }
        div.reddit-post-meta {
            span.reddit-dot {}
            span.reddit-score { (post.score) " pts" }
            span.sep { (SEP) }
            (post.author)
            span.sep { (SEP) }
            time.time-ago data-ts=(created_ts) { (format_time_ago(created_ts)) }
            span.sep { (SEP) }
            a href=(permalink) {
                (post.comment_count) " comments"
            }
            @if !post.is_self {
                span.sep { (SEP) }
                span.reddit-domain { (post.domain) }
            }
        }
    }
}

// Fragment renderers. Each returns a complete <ol> that the client can swap in.

pub fn render_hn_fragment(name: &str, stories: Option<&Vec<HnStory>>, version: u64) -> Markup {
    html! {
        ol.stories data-for-page=(name) data-loaded="true" data-version=(version) {
            li.empty-state { "no stories" }
            @for story in stories.into_iter().flatten() {
                li.story { (render_hn_story(story)) }
            }
        }
    }
}

pub fn render_gh_fragment(
    trending: &GhTrending,
    period: &str,
    lang: &str,
    version: u64,
) -> Option<Markup> {
    let display = lang_display(lang)?;
    let lookup = (period.to_string(), display.to_string());
    Some(html! {
        ol.repos data-for-lang=(lang) data-loaded="true" data-version=(version) {
            li.empty-state { "no repos" }
            @for repo in trending.get(&lookup).into_iter().flatten() {
                li.repo { (render_gh_repo(repo)) }
            }
        }
    })
}

pub fn render_reddit_fragment(feed: &RedditFeed, sub: &str, version: u64) -> Markup {
    let lookup = sub.to_lowercase();
    html! {
        ol.reddit-posts data-for-sub=(sub) data-loaded="true" data-version=(version) {
            li.empty-state { "no posts" }
            @for post in feed.get(lookup.as_str()).into_iter().flatten() {
                li.reddit-post { (render_reddit_post(post)) }
            }
        }
    }
}

pub fn render_gh_custom_fragment(repos: &[TrendingRepo], version: u64) -> Markup {
    html! {
        ol.repos data-for-lang="mine" data-loaded="true" data-custom="true" data-version=(version) {
            li.empty-state { "no repos" }
            @for repo in repos {
                li.repo { (render_gh_repo(repo)) }
            }
        }
    }
}

pub fn render_reddit_custom_fragment(posts: &[RedditPost], version: u64) -> Markup {
    html! {
        ol.reddit-posts data-for-sub="all" data-loaded="true" data-custom="true" data-version=(version) {
            li.empty-state { "no posts" }
            @for post in posts {
                li.reddit-post { (render_reddit_post(post)) }
            }
        }
    }
}

// Collection renderers

/// Selected list first so the no-JS `:first-of-type` rule and the server
/// render agree even when a cookie selects a non-default page.
fn hn_order(view: &View) -> Vec<&'static str> {
    let mut pages: Vec<&'static str> = config::HN_PAGES.iter().map(|&(name, _)| name).collect();
    pages.sort_by_key(|name| (*name != view.hn.as_str()) as u8);
    pages
}

fn lang_order(view: &View) -> Vec<(&'static str, String)> {
    let mut langs: Vec<(&'static str, String)> =
        std::iter::once((config::FILTER_ALL, config::FILTER_ALL.to_string()))
            .chain(
                config::GITHUB_LANGUAGES
                    .iter()
                    .map(|&(name, _)| (name, name.to_lowercase())),
            )
            .collect();
    langs.sort_by_key(|(_, value)| (value != &view.lang) as u8);
    langs
}

fn sub_order(view: &View) -> Vec<&'static str> {
    let mut subs: Vec<&'static str> = std::iter::once(config::FILTER_ALL)
        .chain(config::REDDIT_SUBREDDITS.iter().copied())
        .collect();
    subs.sort_by_key(|sub| (*sub != view.sub.as_str()) as u8);
    subs
}

fn render_hn_pages(hn_pages: &HnPages, view: &View, version: u64) -> Markup {
    html! {
        @for name in hn_order(view) {
            @if name == view.hn {
                (render_hn_fragment(name, hn_pages.get(name), version))
            } @else {
                ol.stories data-for-page=(name) {}
            }
        }
    }
}

fn render_gh_trending(trending: &GhTrending, period: &str, view: &View, version: u64) -> Markup {
    html! {
        @for (_, lang) in lang_order(view) {
            @if period == view.period && lang == view.lang {
                (render_gh_fragment(trending, period, &lang, version)
                    .unwrap_or_else(|| render_gh_placeholder(&lang)))
            } @else {
                (render_gh_placeholder(&lang))
            }
        }
    }
}

fn render_gh_placeholder(lang: &str) -> Markup {
    html! { ol.repos data-for-lang=(lang) {} }
}

fn render_reddit_feed(feed: &RedditFeed, view: &View, version: u64) -> Markup {
    html! {
        @for sub in sub_order(view) {
            @if sub == view.sub {
                (render_reddit_fragment(feed, sub, version))
            } @else {
                ol.reddit-posts data-for-sub=(sub) {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(subreddit: &str, title: &str) -> RedditPost {
        RedditPost {
            title: title.into(),
            url: "https://example.com".into(),
            permalink: "https://reddit.com/r/example".into(),
            subreddit: subreddit.into(),
            score: 1,
            author: "author".into(),
            created_at: 0.0,
            comment_count: 0,
            domain: "example.com".into(),
            is_self: false,
            stickied: false,
        }
    }

    #[test]
    fn language_colors_must_be_hex() {
        for valid in ["#fff", "#dea584", "#00ADD8"] {
            assert!(is_hex_color(valid), "{valid}");
        }
        for invalid in ["", "fff", "#xyz", "#12345", "red", "#1234567"] {
            assert!(!is_hex_color(invalid), "{invalid}");
        }
    }

    #[test]
    fn mixed_case_subreddit_renders_posts() {
        let feed = RedditFeed::from([(
            "claudeai".to_string(),
            vec![post("ClaudeAI", "hello from claude")],
        )]);

        let markup = render_reddit_fragment(&feed, "ClaudeAI", 1).into_string();

        assert!(markup.contains("hello from claude"));
        assert!(markup.contains(r#"data-for-sub="ClaudeAI""#));
    }

    #[test]
    fn unsafe_story_url_falls_back_to_hn() {
        let mut story = crate::providers::hackernews::test_story("unsafe");
        story.url = Some("javascript:alert(1)".into());

        let markup = render_hn_fragment("top", Some(&vec![story]), 1).into_string();

        assert!(markup.contains("https://news.ycombinator.com/item?id=1"));
        assert!(!markup.contains("javascript:"));
    }

    #[test]
    fn unsafe_reddit_url_falls_back_to_permalink() {
        let mut item = post("rust", "unsafe");
        item.url = "data:text/html,boom".into();

        let markup = render_reddit_fragment(
            &RedditFeed::from([("rust".to_string(), vec![item])]),
            "rust",
            1,
        )
        .into_string();

        assert!(markup.contains("https://reddit.com/r/example"));
        assert!(!markup.contains("data:text/html"));
    }

    #[test]
    fn unknown_view_values_fall_back() {
        let view = View {
            hn: "nope".into(),
            period: "yearly".into(),
            lang: "brainfuck".into(),
            sub: "missing".into(),
        }
        .normalized();

        assert_eq!(view, View::default());
    }
}
