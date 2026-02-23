# tty1

A fast, minimal dashboard that aggregates trending content from Hacker News, GitHub, and Reddit into a single page. Built in Rust with server-rendered HTML and no JavaScript framework.

## Features

- **Hacker News** — Top, Newest, and Show HN stories (30 per page)
- **GitHub Trending** — Repos across 17 languages, filterable by daily/weekly/monthly
- **Reddit** — 34 curated subreddits focused on programming and tech
- **PWA** — Installable with offline support via service worker
- **Keyboard shortcuts** — `1`/`2`/`3` to switch panels, arrows to navigate
- **Mobile** — Responsive layout with swipe navigation
- **Fast** — Pre-rendered HTML, gzip/zstd compression, ETag caching, lock-free reads

## Running

### Development

```sh
cargo run
# or with mise
mise run dev
```

The server starts on `http://localhost:3000`. Set `RUST_LOG=tty1=debug,info` for verbose logging.

### Docker

```sh
docker compose up -d
```

Or build and run manually:

```sh
docker build -t tty1 .
docker run --rm -p 3000:3000 tty1
```

Uses a distroless base image with vendored OpenSSL — no system dependencies needed at runtime.

### Testing

```sh
cargo test
```

Tests make live requests to HN, GitHub, and Reddit APIs. Reddit tests may fail due to bot detection.

## Environment Variables

All optional — the app runs with sensible defaults:

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3000` | Server listen port |
| `RUST_LOG` | `info` | Log verbosity (e.g. `tty1=debug,info`) |
| `HTTPS_PROXY` | — | Proxy URL for outbound requests (http/https/socks5/socks5h) |
| `ALL_PROXY` | — | Fallback proxy if `HTTPS_PROXY` not set |

## API

| Endpoint | Description |
|---|---|
| `GET /` | Dashboard HTML (pre-compressed, ETag support) |
| `GET /api/data` | All aggregated data as JSON |
| `GET /api/health` | `200` if data is loaded, `503` while still fetching |

## Architecture

Background workers scrape all three sources every 30 minutes and store the results in an `ArcSwap`-backed shared state. HTML is pre-rendered and pre-compressed (gzip + zstd) every 60 seconds. Incoming requests select the best encoding and return the cached response with ETag support — no rendering happens in the request path.

Each provider fetches concurrently using buffered streams (HN: 10, GitHub: 6, Reddit: 3) and fails independently — one source going down doesn't affect the others. If a fetch returns empty results, the previous data is kept. Transient failures are retried up to 3 times with exponential backoff.

```
Axum server (:3000)
  ├── GET /           → pre-compressed HTML from ArcSwap
  ├── GET /api/data   → JSON snapshot of current data
  └── GET /api/health → 200/503 based on data availability

Background tasks (Tokio)
  ├── Scraper (30 min) → HN API + GitHub HTML + Reddit API → ArcSwap
  └── HTML refresh (1 min) → re-render timestamps → ArcSwap
```

## Configuration

All configuration lives in `src/config.rs` as compile-time constants. Key values:

| Constant | Default | Description |
|---|---|---|
| `DEFAULT_PORT` | `3000` | Server port (overridden by `PORT` env var) |
| `SCRAPE_INTERVAL_SECS` | `1800` | Time between full data refreshes |
| `HTML_REFRESH_SECS` | `60` | Time between HTML re-renders |
| `HN_STORIES_PER_PAGE` | `30` | Stories shown per HN page |
| `GITHUB_REPOS_PER_PAGE` | `25` | Repos shown per GitHub language |
| `REDDIT_POSTS_PER_SUB` | `30` | Posts fetched per subreddit |
| `REDDIT_ALL_VIEW_LIMIT` | `100` | Max posts in merged "all" view |

## License

[MIT](LICENSE)
