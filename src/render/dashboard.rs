use chrono::{DateTime, Utc};
use maud::{Markup, PreEscaped, html};

use super::shell::page_shell;
use super::utils::{SEP, fmt_num, format_time_ago};
use crate::config;
use crate::providers::github::{GhTrending, TrendingRepo};
use crate::providers::hackernews::{HnPages, HnStory};
use crate::providers::reddit::{RedditFeed, RedditPost};

pub fn render_page(
    hn_pages: &HnPages,
    gh_trending: &GhTrending,
    reddit_feed: &RedditFeed,
    last_fetched: DateTime<Utc>,
) -> String {
    page_shell(include_str!("../static/dashboard.css"), html! {
        main #main-content .dashboard {
            section.panel.hn-panel aria-label="Hacker News" {
                header.panel-header {
                    span.source-icon aria-hidden="true" { (PreEscaped(include_str!("../static/icons/hn.svg"))) }
                    a.source-name.hn-link href="https://news.ycombinator.com/" target="_blank" rel="noopener" { "Hackernews" }
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
                    span.source-icon.gh-icon aria-hidden="true" { (PreEscaped(include_str!("../static/icons/github.svg"))) }
                    a.source-name.gh-link href="https://github.com/trending" target="_blank" rel="noopener" { "GitHub Trending" }
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
                            (render_gh_trending(gh_trending, period))
                        }
                    }
                }
            }
            section.panel.reddit-panel aria-label="Reddit" {
                header.panel-header {
                    span.source-icon.reddit-icon aria-hidden="true" { (PreEscaped(include_str!("../static/icons/reddit.svg"))) }
                    a.source-name.reddit-link href="https://www.reddit.com" target="_blank" rel="noopener" { "Reddit" }
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
        @let fetched_ts = last_fetched.timestamp() as u64;
        footer.last-updated aria-live="polite" {
            "updated "
            time.last-updated-time data-ts=(fetched_ts) { (format_time_ago(fetched_ts)) }
            span.sep { (SEP) }
            a.settings-link href="/settings" { "settings" }
        }
        script { (PreEscaped(include_str!("../static/common.js"))) (PreEscaped(include_str!("../static/app.js"))) }
    }).into_string()
}

// Item renderers

fn render_hn_story(story: &HnStory) -> Markup {
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

fn render_gh_repo(repo: &TrendingRepo) -> Markup {
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

fn render_reddit_post(post: &RedditPost) -> Markup {
    let created_ts = post.created_at as u64;
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
            time.time-ago data-ts=(created_ts) { (format_time_ago(created_ts)) }
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

// Collection renderers

fn render_hn_pages(hn_pages: &HnPages) -> Markup {
    html! {
        @for &(name, _) in config::HN_PAGES {
            ol.stories data-for-page=(name) {
                li.empty-state { "no stories" }
                @for story in hn_pages.get(name).into_iter().flatten() {
                    li.story { (render_hn_story(story)) }
                }
            }
        }
    }
}

fn render_gh_trending(trending: &GhTrending, period: &str) -> Markup {
    let lang_keys = std::iter::once(config::FILTER_ALL)
        .chain(config::GITHUB_LANGUAGES.iter().map(|&(name, _)| name));

    html! {
        @for key in lang_keys {
            ol.repos data-for-lang=(key.to_lowercase()) {
                li.empty-state { "no repos" }
                @let lookup = (period.to_string(), key.to_string());
                @for repo in trending.get(&lookup).into_iter().flatten() {
                    li.repo { (render_gh_repo(repo)) }
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
                li.empty-state { "no posts" }
                @for post in feed.get(sub).into_iter().flatten() {
                    li.reddit-post data-sub=(post.subreddit) { (render_reddit_post(post)) }
                }
            }
        }
    }
}
