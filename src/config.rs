// -- App --
pub const PAGE_TITLE: &str = "tty1";
pub const DEFAULT_PORT: u16 = 3000;

pub fn port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}
pub const FILTER_ALL: &str = "all";

// -- Server --
pub const SCRAPE_INTERVAL_SECS: u64 = 1800;
pub const SCRAPE_JITTER_SECS: u64 = 300;
pub const HTML_REFRESH_SECS: u64 = 60;
pub const LOADING_PAGE_TTL_SECS: u64 = 3;
pub const HANDLER_TIMEOUT_SECS: u64 = 10;
pub const GZIP_LEVEL: u32 = 6;
pub const ZSTD_LEVEL: i32 = 3;

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
pub const GITHUB_CONCURRENT_FETCHES: usize = 6;
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
];
pub const REDDIT_CONCURRENT_FETCHES: usize = 3;
pub const REDDIT_POSTS_PER_SUB: usize = 30;
pub const REDDIT_ALL_VIEW_LIMIT: usize = 100;
