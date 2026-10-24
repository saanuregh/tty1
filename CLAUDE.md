# CLAUDE.md

## Build & Run

```sh
cargo run                  # dev server on :3000
cargo test                 # unit tests only (fast)
cargo test -- --ignored    # live API tests (Reddit may 403/429)
cargo build --release      # fast optimized binary (system TLS)
cargo build --release --features vendored-tls  # self-contained binary
docker compose up -d       # build and run with docker compose
mise run frontend          # rebuild src/static/*.bundle.js and *.bundle.css
mise run typecheck         # type-check frontend TypeScript with Bun
mise run browser-test      # Playwright smoke test; needs Playwright or CHROME_PATH
```

Task runner and tool versions live in `.mise.toml`. The `hk` pre-commit hook in
`hk.pkl` invokes `mise run fmt-check` and `mise run lint`, so use those before
committing. Biome config is `biome.jsonc` — it must keep the `.jsonc`
extension, because Biome reads `biome.json` as strict JSON and a comment there
is a configuration error.

Set `RUST_LOG=tty1=debug,info` for verbose logging.

## Environment Variables

All optional — the app runs with sensible defaults and no configuration:

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3000` | Server listen port |
| `RUST_LOG` | `info` | Log verbosity (e.g. `tty1=debug,info`) |
| `REDDIT_PROXY` | — | Proxy URL for Reddit requests only (http/https/socks5/socks5h) |
| `DISABLE_REDDIT` | — | Set to `1` to skip the Reddit worker and ignored Reddit live tests during testing or offline development |
| `DISABLE_SCRAPERS` | — | Set to `1` to skip all background scrapers; used by the browser smoke test |
| `STATE_DIR` | `state` | Directory for the persisted snapshot and Reddit session |

## Project Structure

```
src/
  main.rs          # server setup, routing, signal handling
  routes.rs        # HTTP handlers (index, fragments /f/..., settings, favicon, icon, manifest, sw.js, api/*)
  config.rs        # all constants (ports, intervals, URLs, subreddits, languages) + PORT/STATE_DIR/REDDIT_PROXY env
  csp.rs           # CSP header: SHA-256 hashes of every inline script, built once at startup
  cache.rs         # ArcSwap-backed data, shell, and per-list fragment snapshots; compression; ETags
  state.rs         # persisted feed + Reddit session (versioned JSON, atomic replace, restored at startup)
  worker.rs        # HN/GitHub scraper (8 h, published per provider), Reddit scraper (~2 h), shell refresher (1 min)
  client.rs        # HTTP client (tracing, browser emulation, proxy support)
  render/
    mod.rs         # re-exports: render_page, fragment renderers, View, render_settings_page, render_loading_page
    dashboard.rs   # dashboard shell + per-list fragment templates (panels, items), View selection
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
    icons/         # SVG provider icons + PNG PWA/apple-touch icons
    manifest.json  # PWA web app manifest
    common.css     # shared styles (reset, variables, themes, base, offline)
    dashboard.css  # main page styles (panels, items, loading, mobile)
    settings.css   # settings page styles
    css/
      dashboard.css # @import entry for the dashboard CSS bundle
      settings.css  # @import entry for the settings CSS bundle
    dashboard.bundle.js  # generated inline dashboard script
    dashboard.bundle.css # generated inline dashboard styles
    settings.bundle.js   # generated inline settings script
    settings.bundle.css  # generated inline settings styles
    js/
      common.ts      # localStorage, theme, time formatting, DOM/cookie helpers
      fragments.ts   # fragment URLs, fetch/retry, data version, SW version sync
      filters.ts     # panel visibility, list selection, language prefetch
      profile.ts     # stored profile application and share-URL parsing
      keyboard.ts    # focus/navigation and keyboard handlers
      polling.ts     # time refresh, version poll, offline indicator
      swipe.ts       # mobile panel observer and dots
      dashboard.ts   # dashboard entry point and wiring
      settings.ts    # settings page entry point
    sw.js          # service worker (shell SWR, versioned fragment cache, offline fallback)
tests/
  browser-smoke.js # Playwright UI/CSP/filter/custom-list/axe accessibility test
  fixtures/state/feed.json # deterministic browser-test feed
package.json       # Playwright dev dependency and bun check script
bun.lock           # Bun lockfile
tsconfig.json      # strict browser TypeScript config
```

## Key Patterns

- **ArcSwap for lock-free reads**: `SharedData`, `SharedHtml`, and `SharedFragments` use `Arc<ArcSwap<T>>` so HTTP handlers never block on writer locks. Background tasks atomically swap in new snapshots; fragments are stored before the shell so a poll that sees a new shell version can fetch matching fragments.
- **Pre-rendered shell + per-list fragments**: Every list (`hn:<page>`, `gh:<period>:<lang>`, `reddit:<sub>`) is rendered and gzip/zstd compressed into a `Fragment` when data changes. `GET /` renders only the selected HN page, the selected GH period/language, and the selected Reddit sub; other lists are empty `<ol>` placeholders fetched from `/f/...` when selected. The 1 min refresher re-renders only the shell; fragments stay versioned until data changes.
- **Client fragment cache**: `dashboard.bundle.js` fetches a fragment when `data-loaded` is missing or its `data-version` is stale, swaps the `<ol>` in place (keeping its scroll position), and keeps it in the DOM. An empty list shows CSS skeleton rows while loading; a loaded list keeps its items until the refreshed copy arrives. After idle the page prefetches the other HN pages (the current language already loads in every period tab); languages and subreddits prefetch on select focus. A failed fetch of a visible list retries a few times with growing delays (`FAILURE_RETRY_MS` in `fragments.ts`), and every version poll re-ensures visible lists that are behind. Custom `mine`/filtered `all` lists are requested as private server-rendered fragments and cached only in the browser-local service-worker cache. `syncViewCookie` mirrors `hn`/`period`/`lang`/`sub` into first-party `tty1_*` cookies so the server's first paint matches the user's view.
- **State survives restarts**: `state.rs` writes `feed.json` and `reddit-session.json` under `STATE_DIR` with a same-directory temp file, `fsync`, and `rename`, and loads them before the server binds. Disk writes are queued to a dedicated `state-writer` thread, so `fsync` never blocks a Tokio worker; `main` aborts the background workers and flushes the queue during graceful shutdown. A hard kill or power loss can lose the newest queued snapshot, but the previous atomically written file remains intact. `main` passes the restored `DataSnapshot` to `cache::new_shared_data`, so the first paint has real panels and `/api/health` is `200` immediately. Persisted types are mirrors (`PersistedHnStory`, `PersistedRedditPost`) rather than the provider structs, whose serde attributes skip derived fields. A `version` mismatch, bad JSON, or an expired token logs and starts cold; a save failure never crashes a scrape.
- **Per-key last-good data**: providers return `HashMap<K, Result<Vec<V>, FetchError>>`. `worker::merge_keyed` takes fresh data per key and retains the previous snapshot's value for any key that errored, so one dead subreddit never blanks a panel. A genuinely empty listing is real data, but an empty HN page or a page that loads with no rows reports `FetchError::EmptyPage`. Reddit's merged `"all"` view is derived after the merge (`reddit::build_all_view`) so it draws on retained data too.
- **Reddit-only proxy**: `client::build_clients` returns a direct client and a Reddit client. The direct client calls `.no_proxy()` because reqwest otherwise applies `HTTPS_PROXY` from the environment (`auto_sys_proxy` defaults to true). Only `REDDIT_PROXY` is read.
- **Auto-update without a reload**: `dashboard.bundle.js` polls `/api/version` on an interval, on tab focus, and on `online`, then refetches only visible/custom lists when `last_fetched` advances. No full-page `DOMParser` swap. `sw.js` intercepts `/` only when `request.mode === "navigate"`, so the poll is never answered from the service worker cache; `/f/` is cached by data version and the page prunes older versions.
- **Batched concurrent fetching**: `futures::stream::iter(...).map(fetch).buffered(N)` limits concurrency per provider.
- **Browser emulation**: `client.rs` sets Chrome User-Agent and Client Hints headers on the direct client to avoid bot detection from GitHub. The Reddit client sends no Chrome headers.
- **Reddit as the Android app**: old.reddit and the public `.json` endpoints now require a login. `reddit_auth::RedditSession` gets an anonymous token from the official Android app's OAuth flow (the one Redlib uses) and reads `oauth.reddit.com`. It keeps one random device identity per 24 h token and picks a new one on renewal or after a 401. The old.reddit scraper and public `.json` fetch stay as dead code.
- **Reddit pacing**: `worker::run_reddit_scraper` runs its own loop. Each cycle fetches a shuffled random subset of subreddits (`reddit::plan_cycle`, which always includes subreddits with no posts yet), with uneven gaps and occasional long pauses, and waits out a nearly spent rate-limit window. `worker::merge_reddit` keeps the previous posts of skipped subreddits. Both loops write the snapshot under `SNAPSHOT_WRITE_LOCK`. On a cold start HN publishes after ~2 s and GitHub after its paced request loop; Reddit panels fill after the first Reddit cycle. Each publish re-renders, and the `/api/version` poll swaps the new fragments in.
- **No request retries**: a failed fetch fails fast — provider errors are absorbed by per-key last-good data, so retrying (and hammering rate-limited endpoints like Reddit) buys nothing.
- **OpenSSL, vendored only for release images**: Reddit and GitHub fingerprint TLS and block rustls. Local builds use `native-tls` with the platform TLS stack (OpenSSL on Linux); the `vendored-tls` cargo feature statically links OpenSSL for the distroless Docker image. Release builds use `lto = "thin"` and parallel codegen units; the app is I/O-bound, so fat LTO bought nothing but build time.

## Conventions

- Config is compile-time constants in `config.rs` — only `PORT`, `STATE_DIR`, `REDDIT_PROXY`, `DISABLE_REDDIT`, and `DISABLE_SCRAPERS` are read from env, and `config.rs` is the only file that reads the environment
- HTML is built with Maud macros (compile-time checked), not string templates
- Frontend is strict TypeScript modules and CSS bundled by Bun into one inline script/style per page — no framework and no browser runtime dependency. `mise run frontend` regenerates the committed bundles; `mise run lint` checks they are current and runs `bun check`. CI runs the same checks plus a Playwright smoke test against a fixture state.
- CSS is split by concern (common/dashboard/settings) and inlined into each page's HTML response
- Provider tests make live HTTP requests (not mocked) and are `#[ignore = "live HTTP"]` so the default run stays fast; parsing and merge logic is covered by unit tests
- Security headers (X-Frame-Options, CSP, etc.) are applied via middleware in `routes.rs`. The CSP value is built in `csp.rs`; `script-src` and `style-src` use SHA-256 hashes for the inline bundles; the language dot is an SVG fill attribute, so no inline style attributes are required.

## Common Tasks

- **Add a subreddit**: append to `REDDIT_SUBREDDITS` in `config.rs`; its fragment key is generated automatically
- **Add a GitHub language**: append `("Display Name", "url-slug")` to `GITHUB_LANGUAGES` in `config.rs`; fragment keys and options are generated automatically
- **Add a new HN page**: append `("label", "api_endpoint")` to `HN_PAGES` in `config.rs`; the fragment key and UI filter are generated automatically
- **Change scrape interval**: update `SCRAPE_INTERVAL_SECS` (HN, GitHub) or `REDDIT_SCRAPE_INTERVAL_SECS` in `config.rs`
- **Add a new provider**: create `src/providers/new.rs` returning `HashMap<K, Result<Vec<T>, FetchError>>`, add the stored type to `DataSnapshot` in `cache.rs`, add the fetch, `merge_keyed`, publish/rebuild path in `worker.rs` (and carry the field through `fetch_and_update_reddit`), add a fragment renderer + `/f/` route + `FragmentSnapshot` entries, and add the panel + client fragment key in `render/dashboard.rs` and the relevant `src/static/js/` module
