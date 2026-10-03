# CLAUDE.md

## Build & Run

```sh
cargo run                  # dev server on :3000
cargo test                 # unit tests only (fast)
cargo test -- --ignored    # live API tests (Reddit may 403/429)
cargo build --release      # optimized binary (LTO + strip + oz)
docker compose up -d       # build and run with docker compose
```

Task runner and tool versions live in `.mise.toml`. `mise run lint` and
`mise run fmt-check` are what `.pre-commit-config.yaml` invokes, so use those
before committing. Biome config is `biome.jsonc` — it must keep the `.jsonc`
extension, because `biome.json` rejects comments and silently falls back to
defaults.

Set `RUST_LOG=tty1=debug,info` for verbose logging.

## Environment Variables

All optional — the app runs with sensible defaults and no configuration:

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3000` | Server listen port |
| `RUST_LOG` | `info` | Log verbosity (e.g. `tty1=debug,info`) |
| `REDDIT_PROXY` | — | Proxy URL for Reddit requests only (http/https/socks5/socks5h) |

## Project Structure

```
src/
  main.rs          # server setup, routing, signal handling
  routes.rs        # HTTP handlers (index, settings, favicon, icon, manifest, sw.js, api/health, api/version)
  config.rs        # all constants (ports, intervals, URLs, subreddits, languages) + PORT/REDDIT_PROXY env
  csp.rs           # CSP header: SHA-256 hashes of every inline script, built once at startup
  cache.rs         # ArcSwap-backed shared state, compression (gzip/zstd), ETag
  worker.rs        # HN/GitHub scraper (8 h), Reddit scraper (~2 h), HTML refresher (1 min)
  client.rs        # HTTP client (tracing, browser emulation, proxy support)
  render/
    mod.rs         # re-exports: render_page, render_settings_page, render_loading_page
    dashboard.rs   # main dashboard page template (panels, items)
    settings.rs    # settings page template (theme, panel order, filters)
    shell.rs       # shared HTML shell (head, scripts, meta tags)
    utils.rs       # template helpers (time formatting, number formatting)
  providers/
    mod.rs         # shared error types, batched_fetch helper, test utilities
    hackernews.rs  # HN Firebase API client
    github.rs      # GitHub trending HTML scraper
    reddit.rs      # Reddit OAuth API client (subset, pacing, listing parser)
    reddit_auth.rs # anonymous Android-app OAuth token and device identity
  static/
    icons/         # SVG icons (app, favicon, provider logos) inlined into HTML
    manifest.json  # PWA web app manifest
    common.css     # shared styles (reset, variables, themes, base, offline)
    dashboard.css  # main page styles (panels, items, loading, mobile)
    settings.css   # settings page styles
    common.js      # shared JS (localStorage, theme, time formatting, DOM helpers)
    app.js         # dashboard JS (filtering, keyboard nav, swipe, panel management)
    settings.js    # settings page JS (theme picker, panel order, filter defaults)
    sw.js          # service worker (stale-while-revalidate, offline fallback)
```

## Key Patterns

- **ArcSwap for lock-free reads**: `SharedData` and `SharedHtml` use `Arc<ArcSwap<T>>` so HTTP handlers never block on writer locks. Background tasks atomically swap in new snapshots.
- **Pre-rendered + pre-compressed HTML**: The render step runs on a blocking Tokio task, produces HTML once, then compresses into gzip and zstd variants stored in `HtmlSnapshot`. Request handlers just pick the right variant.
- **Per-key last-good data**: providers return `HashMap<K, Result<Vec<V>, FetchError>>`. `worker::merge_keyed` takes fresh data per key and retains the previous snapshot's value for any key that errored, so one dead subreddit never blanks a panel. An empty `Ok` is real data; a page that loads but yields no rows reports `FetchError::EmptyPage` instead. Reddit's merged `"all"` view is derived after the merge (`reddit::build_all_view`) so it draws on retained data too.
- **Reddit-only proxy**: `client::build_clients` returns a direct client and a Reddit client. The direct client calls `.no_proxy()` because reqwest otherwise applies `HTTPS_PROXY` from the environment (`auto_sys_proxy` defaults to true). Only `REDDIT_PROXY` is read.
- **Auto-update without a reload**: `app.js` polls `/api/version` on an interval, on tab focus, and on `online`, then swaps `.dashboard` in place when `last_fetched` advances. `sw.js` intercepts `/` only when `request.mode === "navigate"`, so the poll is never answered from the service worker cache.
- **Batched concurrent fetching**: `futures::stream::iter(...).map(fetch).buffered(N)` limits concurrency per provider.
- **Browser emulation**: `client.rs` sets Chrome User-Agent and Client Hints headers on the direct client to avoid bot detection from GitHub. The Reddit client sends no Chrome headers.
- **Reddit as the Android app**: old.reddit and the public `.json` endpoints now require a login. `reddit_auth::RedditSession` gets an anonymous token from the official Android app's OAuth flow (the one Redlib uses) and reads `oauth.reddit.com`. It keeps one random device identity per 24 h token and picks a new one on renewal or after a 401. The old.reddit scraper and public `.json` fetch stay as dead code.
- **Reddit pacing**: `worker::run_reddit_scraper` runs its own loop. Each cycle fetches a shuffled random subset of subreddits (`reddit::plan_cycle`, which always includes subreddits with no posts yet), with uneven gaps and occasional long pauses, and waits out a nearly spent rate-limit window. `worker::merge_reddit` keeps the previous posts of skipped subreddits. Both loops write the snapshot under `SNAPSHOT_WRITE_LOCK`. On a cold start the dashboard appears once either loop stores, so Reddit panels stay empty for a few minutes until the first Reddit cycle finishes; the `/api/version` poll swaps them in.
- **No request retries**: a failed fetch fails fast — provider errors are absorbed by per-key last-good data, so retrying (and hammering rate-limited endpoints like Reddit) buys nothing.
- **Vendored OpenSSL**: Reddit and GitHub fingerprint TLS and block rustls. The `native-tls-vendored` feature statically links OpenSSL into the binary.

## Conventions

- Config is compile-time constants in `config.rs` — only `PORT` and `REDDIT_PROXY` are read from env, and `config.rs` is the only file that reads the environment
- HTML is built with Maud macros (compile-time checked), not string templates
- Frontend is vanilla JS — no build step, no framework
- CSS is split by concern (common/dashboard/settings) and inlined into each page's HTML response
- Provider tests make live HTTP requests (not mocked) and are `#[ignore = "live HTTP"]` so the default run stays fast; parsing and merge logic is covered by unit tests
- Security headers (X-Frame-Options, CSP, etc.) are applied via middleware in `routes.rs`. The CSP value is built in `csp.rs`; `style-src` keeps `'unsafe-inline'` and carries no hash on purpose, because a hash in a directive makes browsers ignore `'unsafe-inline'` there and the language-colour dot writes a style attribute that no hash can cover.

## Common Tasks

- **Add a subreddit**: append to `REDDIT_SUBREDDITS` in `config.rs`
- **Add a GitHub language**: append `("Display Name", "url-slug")` to `GITHUB_LANGUAGES` in `config.rs`
- **Add a new HN page**: append `("label", "api_endpoint")` to `HN_PAGES` in `config.rs`, add UI filter in `render/dashboard.rs`
- **Change scrape interval**: update `SCRAPE_INTERVAL_SECS` (HN, GitHub) or `REDDIT_SCRAPE_INTERVAL_SECS` in `config.rs`
- **Add a new provider**: create `src/providers/new.rs` returning `HashMap<K, Result<Vec<T>, FetchError>>`, add the stored type to `DataSnapshot` in `cache.rs`, add the fetch and a `merge_keyed` call to `fetch_and_update` in `worker.rs` (and carry the field through `fetch_and_update_reddit`), add the panel in `render/dashboard.rs`
