use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use maud::{Markup, PreEscaped, html};

use crate::config;
use crate::providers::github::{GhTrending, TrendingRepo};
use crate::providers::hackernews::{HnPages, HnStory};
use crate::providers::reddit::{RedditFeed, RedditPost};

const SEP: PreEscaped<&str> = PreEscaped("\u{00b7}");

fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}

fn page_shell(content: Markup) -> Markup {
    html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="theme-color" content="#0a0a0a";
                meta name="description" content="Tech news dashboard · HN, GitHub Trending, Reddit";
                meta name="apple-mobile-web-app-capable" content="yes";
                meta name="apple-mobile-web-app-status-bar-style" content="black-translucent";
                title { (config::PAGE_TITLE) }
                link rel="icon" type="image/svg+xml" href="/favicon.svg";
                link rel="apple-touch-icon" href="/icon.svg";
                link rel="manifest" href="/manifest.json";
                script { (PreEscaped("document.documentElement.className='js';try{var t=JSON.parse(localStorage.getItem('tty1')||'{}').theme||(matchMedia('(prefers-color-scheme:light)').matches?'light':'dark');document.documentElement.dataset.theme=t}catch(e){}")) }
                style { (PreEscaped(include_str!("static/style.css"))) }
            }
            body {
                a.skip-link href="#main-content" { "Skip to content" }
                .offline-banner.is-hidden { "offline \u{00b7} cached content" }
                (content)
                script {
                    (PreEscaped("if('serviceWorker' in navigator){navigator.serviceWorker.register('/sw.js')}"))
                }
            }
        }
    }
}

pub fn render_page(
    hn_pages: &HnPages,
    gh_trending: &GhTrending,
    reddit_feed: &RedditFeed,
    last_fetched: DateTime<Utc>,
) -> String {
    page_shell(html! {
        main #main-content .dashboard {
            section.panel.hn-panel aria-label="Hacker News" {
                header.panel-header {
                    span.source-icon aria-hidden="true" { (PreEscaped(include_str!("static/icons/hn.svg"))) }
                    span.source-name { "Hackernews" }
                    select.hn-select aria-label="HN page" {
                        @for (i, &(name, _)) in config::HN_PAGES.iter().enumerate() {
                            option value=(name) selected[i == 0] { (name) }
                        }
                    }
                }
                (render_hn_pages(hn_pages))
            }
            section.panel.gh-panel aria-label="GitHub Trending" {
                header.panel-header {
                    span.source-icon.gh-icon aria-hidden="true" { (PreEscaped(include_str!("static/icons/github.svg"))) }
                    span.source-name { "GitHub Trending" }
                    .tab-labels role="tablist" aria-label="Trending period" {
                        @for (i, &period) in config::GITHUB_PERIODS.iter().enumerate() {
                            input id=(format!("gh-tab-{period}"))
                                  type="radio" name="gh-tab"
                                  checked[i == 0];
                            label .active[i == 0]
                                  for=(format!("gh-tab-{period}"))
                                  role="tab"
                                  aria-selected=(if i == 0 { "true" } else { "false" })
                                  aria-controls=(format!("gh-{period}"))
                                  { (&period[..1]) }
                        }
                    }
                    select.lang-select aria-label="Programming language" {
                        option value=(config::FILTER_ALL) selected { (config::FILTER_ALL) }
                        @for &(name, _) in config::GITHUB_LANGUAGES {
                            option value=(name.to_lowercase()) { (name) }
                        }
                    }
                }
                .gh-tabs {
                    @for (i, &period) in config::GITHUB_PERIODS.iter().enumerate() {
                        .tab-content .active[i == 0]
                            id=(format!("gh-{period}"))
                            role="tabpanel"
                            aria-labelledby=(format!("gh-tab-{period}"))
                        {
                            (render_period_repos(gh_trending, period))
                        }
                    }
                }
            }
            section.panel.reddit-panel aria-label="Reddit" {
                header.panel-header {
                    span.source-icon.reddit-icon aria-hidden="true" { (PreEscaped(include_str!("static/icons/reddit.svg"))) }
                    span.source-name { "Reddit" }
                    select.subreddit-select aria-label="Subreddit" {
                        option value=(config::FILTER_ALL) selected { (config::FILTER_ALL) }
                        @for sub in config::REDDIT_SUBREDDITS {
                            option value=(sub) { "r/" (sub) }
                        }
                    }
                }
                (render_reddit_feed(reddit_feed))
            }
        }
        nav.swipe-dots aria-label="Panel navigation" {
            button.swipe-dot.dot-hn.active type="button" aria-label="Hacker News" {}
            button.swipe-dot.dot-gh type="button" aria-label="GitHub" {}
            button.swipe-dot.dot-reddit type="button" aria-label="Reddit" {}
        }
        footer.last-updated aria-live="polite" {
            "updated "
            time.last-updated-time data-ts=(last_fetched.timestamp()) { (format_time_ago(last_fetched.timestamp() as u64)) }
            span.sep { (SEP) }
            button.theme-toggle type="button" aria-label="Toggle theme" { "\u{25d0}" }
            span.sep { (SEP) }
            a.settings-link href="/settings" { "settings" }
        }
        script { (PreEscaped(include_str!("static/state.js"))) }
    }).into_string()
}

fn render_story(story: &HnStory) -> Markup {
    let link_url = story.url.as_deref().unwrap_or(&story.hn_url);

    html! {
        span.story-title {
            a href=(link_url) {
                (story.title)
            }
            @if let Some(ref domain) = story.domain {
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
            a href=(story.hn_url) {
                (story.comment_count) " comments"
            }
        }
    }
}

fn render_repo(repo: &TrendingRepo) -> Markup {
    html! {
        span.repo-title {
            a href=(repo.url) {
                span.repo-author { (&repo.author) "/" }
                (&repo.name)
            }
        }
        @if !repo.description.is_empty() {
            p.repo-desc { (&repo.description) }
        }
        div.repo-meta {
            @if let Some(ref lang) = repo.language {
                @if let Some(ref color) = repo.language_color {
                    span.lang-dot style=(format!("background:{color}")) {}
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

fn render_hn_pages(hn_pages: &HnPages) -> Markup {
    html! {
        @for &(name, _) in config::HN_PAGES {
            ol.stories data-for-page=(name) {
                @for story in hn_pages.get(name).into_iter().flatten() {
                    li.story { (render_story(story)) }
                }
            }
        }
    }
}

fn render_period_repos(trending: &GhTrending, period: &str) -> Markup {
    let lang_keys = std::iter::once(config::FILTER_ALL)
        .chain(config::GITHUB_LANGUAGES.iter().map(|&(name, _)| name));

    html! {
        @for key in lang_keys {
            ol.repos data-for-lang=(key.to_lowercase()) {
                @let lookup = (period.to_string(), key.to_string());
                @for repo in trending.get(&lookup).into_iter().flatten() {
                    li.repo { (render_repo(repo)) }
                }
            }
        }
    }
}

fn render_reddit_feed(feed: &RedditFeed) -> Markup {
    let subs = std::iter::once(config::FILTER_ALL).chain(config::REDDIT_SUBREDDITS.iter().copied());

    html! {
        @for sub in subs {
            ol.reddit-posts data-for-sub=(sub) {
                @for post in feed.get(sub).into_iter().flatten() {
                    li.reddit-post { (render_reddit_post(post)) }
                }
            }
        }
    }
}

fn render_reddit_post(post: &RedditPost) -> Markup {
    html! {
        span.reddit-post-title {
            a href=(post.url) {
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
            time.time-ago data-ts=(post.created_at as u64) { (format_time_ago(post.created_at as u64)) }
            span.sep { (SEP) }
            a href=(post.permalink) {
                (post.comment_count) " comments"
            }
            @if !post.is_self {
                span.sep { (SEP) }
                span.reddit-domain { (post.domain) }
            }
        }
    }
}

pub fn render_loading_page() -> String {
    page_shell(html! {
        .loading {
            .loading-spinner {}
            .loading-text { "fetching sources" }
            .loading-sources {
                span.loading-src.hn-accent {
                    (PreEscaped(include_str!("static/icons/hn.svg")))
                    "hackernews"
                }
                span.loading-sep { (SEP) }
                span.loading-src.gh-accent {
                    (PreEscaped(include_str!("static/icons/github.svg")))
                    "github"
                }
                span.loading-sep { (SEP) }
                span.loading-src.reddit-accent {
                    (PreEscaped(include_str!("static/icons/reddit.svg")))
                    "reddit"
                }
            }
        }
    })
    .into_string()
}

pub fn render_settings_page() -> String {
    page_shell(html! {
        main #main-content .settings {
            header.settings-header {
                a href="/" { "← tty1" }
                h1 { "settings" }
            }

            section.settings-section {
                span.settings-label { "theme" }
                div.theme-buttons {
                    button.theme-btn data-theme="dark" { "dark" }
                    button.theme-btn data-theme="light" { "light" }
                }
            }

            section.settings-section {
                span.settings-label { "keyboard shortcuts" }
                div.keybind-list {
                    div.keybind-row {
                        span.keybind-key { "h" }
                        span.keybind-key { "l" }
                        span.keybind-desc { "switch panels" }
                    }
                    div.keybind-row {
                        span.keybind-key { "j" }
                        span.keybind-key { "k" }
                        span.keybind-desc { "navigate items" }
                    }
                    div.keybind-row {
                        span.keybind-key { "f" }
                        span.keybind-desc { "focus filter" }
                    }
                    div.keybind-row {
                        span.keybind-key { "Enter" }
                        span.keybind-desc { "open link" }
                    }
                    div.keybind-row {
                        span.keybind-key { "Shift+Enter" }
                        span.keybind-desc { "open in new tab" }
                    }
                    div.keybind-row {
                        span.keybind-key { "t" }
                        span.keybind-desc { "toggle theme" }
                    }
                    div.keybind-row {
                        span.keybind-key { "Escape" }
                        span.keybind-desc { "unfocus" }
                    }
                }
            }

            section.settings-section {
                button.reset-btn { "reset to defaults" }
                small.muted { "clears all saved preferences" }
            }
        }
        script { (PreEscaped(include_str!("static/settings.js"))) }
    })
    .into_string()
}

// Duplicated in static/state.js for client-side refresh — keep both in sync.
const TIME_UNITS: &[(u64, &str)] = &[
    (31536000, "y"),
    (2592000, "mo"),
    (604800, "w"),
    (86400, "d"),
    (3600, "h"),
    (60, "m"),
];

fn format_time_ago(unix_time: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let elapsed = now.saturating_sub(unix_time);

    for &(secs, suffix) in TIME_UNITS {
        let count = elapsed / secs;
        if count > 0 {
            return format!("{count}{suffix}");
        }
    }
    "0m".to_string()
}
