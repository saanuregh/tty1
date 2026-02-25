# CLAUDE.md

## Build & Run

```sh
cargo run                  # dev server on :3000
cargo test                 # live API tests (Reddit may 403)
cargo build --release      # optimized binary (LTO + strip + oz)
docker compose up -d       # build and run with docker compose
```

Set `RUST_LOG=tty1=debug,info` for verbose logging.

## Environment Variables

All optional — the app runs with sensible defaults and no configuration:

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3000` | Server listen port |
| `RUST_LOG` | `info` | Log verbosity (e.g. `tty1=debug,info`) |
| `HTTPS_PROXY` | — | Proxy URL (http/https/socks5/socks5h) |
| `ALL_PROXY` | — | Fallback proxy if `HTTPS_PROXY` not set |

## Project Structure

```
src/
  main.rs          # server setup, routing, signal handling
  routes.rs        # HTTP handlers (index, favicon, icon, manifest, sw.js, api/data, api/health)
  config.rs        # all constants (ports, intervals, URLs, subreddits, languages)
  cache.rs         # ArcSwap-backed shared state, compression (gzip/zstd), ETag
  render.rs        # Maud HTML templates (full page, panels, loading state)
  worker.rs        # background scraper (30 min) and HTML refresher (1 min)
  client.rs        # HTTP client (retry, tracing, browser emulation, proxy support)
  providers/
    mod.rs         # shared error types, batched_fetch helper, test utilities
    hackernews.rs  # HN Firebase API client
    github.rs      # GitHub trending HTML scraper
    reddit.rs      # Reddit JSON API client
  static/
    icons/         # SVG icons (app, favicon, provider logos) inlined into HTML
    manifest.json  # PWA web app manifest
    common.css     # shared styles (reset, variables, themes, base, offline)
    dashboard.css  # main page styles (panels, items, loading, mobile)
    settings.css   # settings page styles
    app.js         # client-side filtering, localStorage, keyboard shortcuts, swipe nav
    sw.js          # service worker (stale-while-revalidate, offline fallback)
```

## Key Patterns

- **ArcSwap for lock-free reads**: `SharedData` and `SharedHtml` use `Arc<ArcSwap<T>>` so HTTP handlers never block on writer locks. Background tasks atomically swap in new snapshots.
- **Pre-rendered + pre-compressed HTML**: The render step runs on a blocking Tokio task, produces HTML once, then compresses into gzip and zstd variants stored in `HtmlSnapshot`. Request handlers just pick the right variant.
- **Independent provider failure**: All three providers run via `tokio::join!`. If one fails, its data stays unchanged (`keep_if_empty` pattern). Errors are logged, not propagated.
- **Batched concurrent fetching**: `futures::stream::iter(...).map(fetch).buffered(N)` limits concurrency per provider.
- **Browser emulation**: `client.rs` sets Chrome User-Agent and Client Hints headers to avoid bot detection from GitHub and Reddit.
- **Retry with backoff**: `reqwest-retry` middleware retries transient failures up to 3 times with exponential backoff.
- **Vendored OpenSSL**: Reddit and GitHub fingerprint TLS and block rustls. The `native-tls-vendored` feature statically links OpenSSL into the binary.

## Conventions

- Config is compile-time constants in `config.rs` — only PORT and proxy are read from env
- HTML is built with Maud macros (compile-time checked), not string templates
- Frontend is vanilla JS (200 lines) — no build step, no framework
- CSS is split by concern (common/dashboard/settings) and inlined into each page's HTML response
- Provider tests make live HTTP requests (not mocked)
- Security headers (X-Frame-Options, CSP, etc.) are applied via middleware in `routes.rs`

## Common Tasks

- **Add a subreddit**: append to `REDDIT_SUBREDDITS` in `config.rs`
- **Add a GitHub language**: append `("Display Name", "url-slug")` to `GITHUB_LANGUAGES` in `config.rs`
- **Add a new HN page**: append `("label", "api_endpoint")` to `HN_PAGES` in `config.rs`, add UI filter in `render.rs`
- **Change scrape interval**: update `SCRAPE_INTERVAL_SECS` in `config.rs`
- **Add a new provider**: create `src/providers/new.rs`, add data type to `DataSnapshot` in `cache.rs`, add fetch call to `scrape_all` in `worker.rs`, add panel in `render.rs`
