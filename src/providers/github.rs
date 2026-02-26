use std::collections::HashMap;
use std::sync::LazyLock;

use crate::client::Client;
use futures::stream::{self, StreamExt};
use scraper::{Html, Selector};

use super::FetchError;
use crate::config;

struct Selectors {
    article: Selector,
    h2_a: Selector,
    desc: Selector,
    lang: Selector,
    lang_color: Selector,
    link: Selector,
    period: Selector,
}

static SELECTORS: LazyLock<Option<Selectors>> = LazyLock::new(|| {
    let selectors = Some(Selectors {
        article: Selector::parse("article.Box-row").ok()?,
        h2_a: Selector::parse("h2 a").ok()?,
        desc: Selector::parse("p.col-9").ok()?,
        lang: Selector::parse("span[itemprop='programmingLanguage']").ok()?,
        lang_color: Selector::parse("span.repo-language-color").ok()?,
        link: Selector::parse(".f6.color-fg-muted a").ok()?,
        period: Selector::parse(".d-inline-block.float-sm-right").ok()?,
    });
    if selectors.is_none() {
        tracing::error!("github: CSS selectors failed to parse — trending scraper is broken");
    }
    selectors
});

/// Key: (period, language) e.g. ("daily", "all"), ("weekly", "Rust")
pub type GhTrending = HashMap<(String, String), Vec<TrendingRepo>>;

#[derive(Debug, Clone)]
pub struct TrendingRepo {
    pub author: String,
    pub name: String,
    pub url: String,
    pub description: String,
    pub language: Option<String>,
    pub language_color: Option<String>,
    pub stars: u64,
    pub forks: u64,
    pub period_stars: String,
}

/// GitHub has no public trending API — HTML scraping is the only option.
pub async fn fetch_trending(
    client: &Client,
    since: &str,
    language: Option<&str>,
) -> Result<Vec<TrendingRepo>, FetchError> {
    let lang_segment = language.map_or(String::new(), |l| format!("/{l}"));
    let url = format!(
        "{}{}?since={}",
        config::GITHUB_TRENDING_URL,
        lang_segment,
        since
    );
    let html_text = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // spawn_blocking: scraper HTML parsing is CPU-bound.
    let repos = tokio::task::spawn_blocking(move || parse_trending_html(&html_text)).await?;
    Ok(repos)
}

pub async fn fetch_all_trending(client: &Client) -> GhTrending {
    tracing::info!(
        languages = config::GITHUB_LANGUAGES.len(),
        "github: fetching"
    );

    let languages = std::iter::once((config::FILTER_ALL, None)).chain(
        config::GITHUB_LANGUAGES
            .iter()
            .map(|&(name, slug)| (name, Some(slug))),
    );

    let futs: Vec<_> = languages
        .flat_map(|(name, slug)| {
            config::GITHUB_PERIODS.iter().map(move |&period| {
                let client = client.clone();
                async move {
                    let repos = fetch_trending(&client, period, slug)
                        .await
                        .unwrap_or_else(|e| {
                            tracing::warn!(error = %e, "github trending {period} {name}");
                            Vec::new()
                        });
                    ((period.to_string(), name.to_string()), repos)
                }
            })
        })
        .collect();

    stream::iter(futs)
        .buffered(config::GITHUB_CONCURRENT_FETCHES)
        .collect()
        .await
}

fn text(el: scraper::ElementRef) -> String {
    el.text().collect::<String>().trim().to_string()
}

fn parse_num(s: &str) -> u64 {
    s.bytes().fold(0u64, |acc, b| {
        if b.is_ascii_digit() {
            acc * 10 + (b - b'0') as u64
        } else {
            acc
        }
    })
}

fn parse_trending_html(html: &str) -> Vec<TrendingRepo> {
    let Some(sel) = SELECTORS.as_ref() else {
        tracing::error!("CSS selectors failed to parse");
        return Vec::new();
    };

    let document = Html::parse_document(html);
    let mut repos = Vec::with_capacity(config::GITHUB_REPOS_PER_PAGE);

    // Detect stale selectors: a valid trending page always has article rows.
    // If the page loaded but nothing matched, the HTML structure likely changed.
    if document.select(&sel.article).next().is_none() && html.len() > 1000 {
        tracing::error!(
            html_len = html.len(),
            "github: 0 articles matched on non-empty page — CSS selectors may be stale"
        );
        return repos;
    }

    for article in document.select(&sel.article) {
        let Some(repo_link) = article.select(&sel.h2_a).next() else {
            continue;
        };
        let href = repo_link.value().attr("href").unwrap_or_default();
        let Some((author, name)) = href.trim_matches('/').split_once('/') else {
            continue;
        };
        let url = format!("https://github.com{href}");

        let description = article
            .select(&sel.desc)
            .next()
            .map(text)
            .unwrap_or_default();
        let language = article.select(&sel.lang).next().map(text);

        let language_color = article
            .select(&sel.lang_color)
            .next()
            .and_then(|el| el.value().attr("style"))
            .and_then(|s| s.strip_prefix("background-color: "))
            .map(String::from);

        let mut links = article.select(&sel.link);
        let stars = links.next().map(text).map_or(0, |s| parse_num(&s));
        let forks = links.next().map(text).map_or(0, |s| parse_num(&s));
        let period_stars = article
            .select(&sel.period)
            .next()
            .map(text)
            .unwrap_or_default();

        if stars == 0 && forks == 0 {
            tracing::warn!(
                repo = %format!("{author}/{name}"),
                "parsed with 0 stars and 0 forks — sub-selectors may be stale"
            );
        }

        repos.push(TrendingRepo {
            author: author.to_string(),
            name: name.to_string(),
            url,
            description,
            language,
            language_color,
            stars,
            forks,
            period_stars,
        });
    }

    repos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::tests::test_client;

    #[tokio::test]
    async fn fetch_trending_all_periods() {
        let client = test_client();
        for &period in config::GITHUB_PERIODS {
            let repos = fetch_trending(&client, period, None).await.unwrap();
            assert!(!repos.is_empty(), "{period} trending returned no repos");
            for repo in &repos {
                assert!(!repo.author.is_empty());
                assert!(!repo.name.is_empty());
                assert!(repo.url.starts_with("https://github.com/"));
                assert!(repo.stars > 0 || repo.forks > 0, "missing stars/forks");
            }
        }
    }
}
