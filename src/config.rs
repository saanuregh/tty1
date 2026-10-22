// -- App --
use std::path::PathBuf;

pub const PAGE_TITLE: &str = "tty1";
pub const DEFAULT_PORT: u16 = 3000;
pub const DEFAULT_STATE_DIR: &str = "state";

pub fn port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}
/// Proxy for Reddit requests only. Hacker News and GitHub always go direct.
pub fn reddit_proxy() -> Option<String> {
    proxy_from(std::env::var("REDDIT_PROXY").ok())
}

/// Split from `reddit_proxy` so it is testable without mutating process env.
fn proxy_from(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// Directory holding the persisted snapshot and Reddit session. Optional: an
/// unwritable path only costs a cold start, never a crash.
pub fn state_dir() -> PathBuf {
    state_dir_from(std::env::var("STATE_DIR").ok())
}

fn state_dir_from(value: Option<String>) -> PathBuf {
    value
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR))
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

/// Testing escape hatch: skip the Reddit worker entirely. Useful for local UI
/// checks without touching Reddit's rate limits.
pub fn reddit_scraper_enabled() -> bool {
    !env_flag("DISABLE_REDDIT")
}

/// Testing escape hatch: skip all background scrapers, including HN/GitHub.
pub fn scrapers_enabled() -> bool {
    !env_flag("DISABLE_SCRAPERS")
}

pub const FILTER_ALL: &str = "all";

// -- Server --
pub const SCRAPE_INTERVAL_SECS: u64 = 28800;
pub const SCRAPE_JITTER_SECS: u64 = 300;
pub const HTML_REFRESH_SECS: u64 = 60;
pub const LOADING_PAGE_TTL_SECS: u64 = 3;
pub const HANDLER_TIMEOUT_SECS: u64 = 10;
pub const REQUEST_TIMEOUT_SECS: u64 = 10;
pub const MAX_CONCURRENT_RENDERS: usize = 4;
pub const CUSTOM_PAGE_CACHE_ENTRIES: usize = 32;
pub const CUSTOM_FRAGMENT_CACHE_ENTRIES: usize = 64;
pub const GZIP_LEVEL: u32 = 6;
pub const ZSTD_LEVEL: i32 = 6;

// -- Hacker News --
pub const HN_API_BASE: &str = "https://hacker-news.firebaseio.com/v0";
pub const HN_STORIES_PER_PAGE: usize = 30;
pub const HN_CONCURRENT_FETCHES: usize = 10;
pub const HN_PAGES: &[(&str, &str)] = &[
    ("top", "topstories"),
    ("newest", "newstories"),
    ("show", "showstories"),
];

// -- GitHub --
pub const GITHUB_TRENDING_URL: &str = "https://github.com/trending";
/// GitHub applies secondary limits to bursty unauthenticated scraping; pace requests.
/// More tolerant than Reddit, so faster (2/s) — still finishes under the Reddit leg.
pub const GITHUB_REQUEST_INTERVAL_MS: u64 = 500;
/// Randomize each gap by ±this so the cadence is not a fixed (bot-detectable) interval.
pub const GITHUB_REQUEST_JITTER_MS: u64 = 150;
pub const GITHUB_REPOS_PER_PAGE: usize = 25;
pub const GITHUB_PERIODS: &[&str] = &["daily", "weekly", "monthly"];
pub const GITHUB_LANGUAGES: &[(&str, &str)] = &[
    ("Rust", "rust"),
    ("Go", "go"),
    ("Zig", "zig"),
    ("TypeScript", "typescript"),
    ("JavaScript", "javascript"),
    ("Python", "python"),
    ("C", "c"),
    ("C++", "c++"),
    ("Lua", "lua"),
    ("Dart", "dart"),
    ("Nix", "nix"),
    ("Java", "java"),
    ("C#", "c%23"),
    ("CSS", "css"),
    ("HTML", "html"),
    ("Shell", "shell"),
    ("Elixir", "elixir"),
];

// -- Reddit --
pub const REDDIT_SUBREDDITS: &[&str] = &[
    "rust",
    "golang",
    "zig",
    "typescript",
    "javascript",
    "python",
    "selfhosted",
    "archlinux",
    "linux",
    "ClaudeAI",
    "neovim",
    "webdev",
    "programming",
    "NixOS",
    "devops",
    "cpp",
    "homelab",
    "claudecode",
    "java",
    "csharp",
    "reactjs",
    "nextjs",
    "sveltejs",
    "vuejs",
    "css",
    "tailwindcss",
    "Frontend",
    "elixir",
    "node",
    "bun",
    "FlutterDev",
    "dotnet",
    "OpenAI",
    "artificial",
    "aws",
    "docker",
    "netsec",
];
/// Reddit runs its own loop, shorter than `SCRAPE_INTERVAL_SECS`, so a random
/// subset per cycle still keeps every subreddit reasonably fresh.
pub const REDDIT_SCRAPE_INTERVAL_SECS: u64 = 7200;
pub const REDDIT_SCRAPE_JITTER_SECS: u64 = 900;
/// Each cycle fetches a random share of `REDDIT_SUBREDDITS`, in this range.
pub const REDDIT_SUBSET_MIN_PERCENT: u64 = 60;
pub const REDDIT_SUBSET_MAX_PERCENT: u64 = 80;
/// Gap between requests: uniform in this range, or occasionally a long
/// "reading" pause, so the cadence looks like someone browsing.
pub const REDDIT_GAP_MIN_MS: u64 = 2000;
pub const REDDIT_GAP_MAX_MS: u64 = 8000;
pub const REDDIT_LONG_PAUSE_PERCENT: u64 = 10;
pub const REDDIT_LONG_PAUSE_MIN_MS: u64 = 20_000;
pub const REDDIT_LONG_PAUSE_MAX_MS: u64 = 60_000;
/// At or below this many remaining requests, wait for the rate-limit window to reset.
pub const REDDIT_RATE_LIMIT_LOW: f64 = 10.0;
pub const REDDIT_RATE_LIMIT_MAX_PAUSE_SECS: u64 = 600;
/// Anonymous token flow of the official Android app (as used by Redlib).
/// Reddit's public JSON and old.reddit both require a login now; this does not.
pub const REDDIT_TOKEN_URL: &str = "https://www.reddit.com/auth/v2/oauth/access-token/loid";
pub const REDDIT_API_BASE: &str = "https://oauth.reddit.com";
pub const REDDIT_ANDROID_CLIENT_ID: &str = "ohXpoqrZYub1kg";
pub const REDDIT_ANDROID_APP_VERSIONS: &[&str] = &[
    "Version 2024.22.1/Build 1652272",
    "Version 2024.23.1/Build 1665606",
    "Version 2024.24.1/Build 1682520",
];
/// Tokens last 24 h; renew this long before expiry.
pub const REDDIT_TOKEN_RENEW_MARGIN_SECS: u64 = 3600;
/// After a failed renewal, keep the current token and retry after this long.
pub const REDDIT_TOKEN_RENEW_RETRY_SECS: u64 = 600;
/// Upper bound on a token's trusted lifetime, whatever `expires_in` says.
pub const REDDIT_TOKEN_MAX_LIFETIME_SECS: u64 = 7 * 86_400;
pub const REDDIT_POSTS_PER_SUB: usize = 30;
pub const REDDIT_ALL_VIEW_LIMIT: usize = 100;
/// Upper bound for a private custom list, regardless of selected sources.
pub const CUSTOM_LIST_LIMIT: usize = 100;
