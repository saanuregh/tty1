use maud::{Markup, PreEscaped, html};

use super::utils::SEP;
use crate::config;

/// Runs before first paint to avoid a flash of the wrong theme.
pub const THEME_BOOTSTRAP: &str = "document.documentElement.className='js';try{var t=JSON.parse(localStorage.getItem('tty1')||'{}').theme||(matchMedia('(prefers-color-scheme:light)').matches?'light':'dark');document.documentElement.dataset.theme=t;var m=document.querySelector('meta[name=theme-color]');if(m)m.setAttribute('content',t==='light'?'#f5f5f5':'#0a0a0a')}catch(e){}";

pub const SW_REGISTER: &str =
    "if('serviceWorker' in navigator){navigator.serviceWorker.register('/sw.js')}";

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
                link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png";
                link rel="manifest" href="/manifest.json";
                script { (PreEscaped(THEME_BOOTSTRAP)) }
                style { (PreEscaped(page_css)) }
            }
            body {
                a.skip-link href="#main-content" { "Skip to content" }
                aside.offline-banner.is-hidden aria-label="Connection status" aria-hidden="true" { "offline \u{00b7} cached content" }
                (content)
                script { (PreEscaped(SW_REGISTER)) }
            }
        }
    }
}

pub fn render_loading_page() -> String {
    page_shell(
        super::DASHBOARD_CSS,
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
