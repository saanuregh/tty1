use maud::{Markup, PreEscaped, html};

use super::utils::SEP;
use crate::config;

pub fn page_shell(page_css: &str, content: Markup) -> Markup {
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
                style { (PreEscaped(include_str!("../static/common.css"))) (PreEscaped(page_css)) }
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

pub fn render_loading_page() -> String {
    page_shell(
        include_str!("../static/dashboard.css"),
        html! {
            .loading {
                .loading-spinner {}
                .loading-text { "fetching sources" }
                .loading-sources {
                    span.loading-src.hn-accent {
                        (PreEscaped(include_str!("../static/icons/hn.svg")))
                        "hackernews"
                    }
                    span.loading-sep { (SEP) }
                    span.loading-src.gh-accent {
                        (PreEscaped(include_str!("../static/icons/github.svg")))
                        "github"
                    }
                    span.loading-sep { (SEP) }
                    span.loading-src.reddit-accent {
                        (PreEscaped(include_str!("../static/icons/reddit.svg")))
                        "reddit"
                    }
                }
            }
        },
    )
    .into_string()
}
