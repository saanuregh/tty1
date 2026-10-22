# tty1

A fast, minimal dashboard that aggregates trending content from Hacker News, GitHub, and Reddit into a single page. Built in Rust with server-rendered HTML and no JavaScript framework. No tracking, no analytics, minimal TypeScript bundled into one inline script with Bun, and offline support for the shell and lists you have opened. The first page is a pre-compressed shell with only the selected lists; other filter lists are fetched as small, independently cached fragments.

![screenshot](.github/screenshot.png)

## Features

- **Hacker News** — Top, Newest, and Show HN stories (30 per page)
- **GitHub Trending** — Repos across 17 languages, filterable by daily/weekly/monthly
- **Reddit** — 37 curated subreddits focused on programming and tech
- **PWA** — Installable with offline support via service worker
- **Keyboard shortcuts** — vim-style navigation (`h/l` panels, `j/k` items, `f` filters, `Enter`/`c` open)
- **Settings** — theme, panel order, default filters, persisted in localStorage, shareable via URL
- **Mobile** — Responsive layout with swipe navigation
- **Fast** — Pre-rendered shell and list fragments, gzip/zstd compression, ETag caching, lock-free reads

## Running

### Development

```sh
cargo run
# or with mise
mise run dev
```

The server starts on `http://localhost:3000`. Set `RUST_LOG=tty1=debug,info` for verbose logging.

Frontend source lives in `src/static/js/` (TypeScript) and `src/static/css/`. After changing it, run `mise run frontend` to rebuild the committed `src/static/*.bundle.js` and `src/static/*.bundle.css` files; `mise run typecheck` runs `bun check`, and `mise run lint` runs both the type check and the stale-bundle check.

`cargo build --release` uses the platform native TLS (OpenSSL on Linux) and is the fast local build. Add `--features vendored-tls` to statically link OpenSSL into a self-contained binary; the Docker image uses that feature.

### Docker

```sh
docker compose up -d
```

Or build and run manually:

```sh
docker build -t tty1 .
docker run --rm -p 3000:3000 tty1
```

Uses a distroless base image with vendored OpenSSL — no system dependencies needed at runtime. The server runs as uid `65532`; Compose runs a one-shot `state-init` service to make the `/state` volume writable before the app starts. Compose mounts that named volume at `/state`, so the last snapshot and the Reddit session survive container re-creation. Running the image directly starts cold unless you mount a volume at `STATE_DIR`.

If you run the image directly with a volume created by an older root-owned image, fix its ownership once:

```sh
docker run --rm -v tty1-state:/state busybox chown -R 65532:65532 /state
```

### Testing

```sh
cargo test              # unit tests only (fast)
cargo test -- --ignored # live requests to HN, GitHub, and Reddit
```

The live tests are ignored by default so the normal test run stays fast and deterministic. Reddit can answer `403` or `429` from datacenter addresses. For manual UI testing, set `DISABLE_REDDIT=1` to skip the Reddit worker entirely and avoid its rate limits. Set `DISABLE_SCRAPERS=1` to skip all background workers; the Playwright smoke test uses this with a fixture state and also runs axe accessibility checks in both themes:

```sh
bun install --frozen-lockfile
bunx playwright install chromium   # once per machine, if no CHROME_PATH is set
mise run browser-test
```

The frontend type check is `mise run typecheck`; it uses `bun check` and needs no Node.js or separate TypeScript dependency.

On systems with a system Chromium, set `CHROME_PATH=/path/to/chromium` instead of installing Playwright's browser.

## Environment Variables

All optional — the app runs with sensible defaults:

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3000` | Server listen port |
| `RUST_LOG` | `info` | Log verbosity (e.g. `tty1=debug,info`) |
| `REDDIT_PROXY` | — | Proxy URL for Reddit requests only (http/https/socks5/socks5h) |
| `DISABLE_REDDIT` | — | Set to `1` to skip the Reddit worker and ignored Reddit live tests during testing or offline development |
| `DISABLE_SCRAPERS` | — | Set to `1` to skip all background scrapers; used by the browser smoke test |
| `STATE_DIR` | `state` | Directory for the persisted snapshot and Reddit session. An unwritable path only costs a cold start |

## API

| Endpoint | Description |
|---|---|
| `GET /` | Shell + the selected lists (or a cookie/query view), pre-compressed with ETag support |
| `GET /f/hn/{page}` | One Hacker News list fragment |
| `GET /f/gh/{period}/{lang}` | One GitHub Trending list fragment |
| `GET /f/reddit/{sub}` | One subreddit list fragment |
| `GET /f/reddit/all?subs=…` | Private merged list for selected subreddits |
| `GET /f/gh/{period}/mine?langs=…` | Private merged list for selected languages |
| `GET /settings` | Settings page (theme, panel order, filters) |
| `GET /api/health` | `200` if data is loaded, `503` while still fetching |
| `GET /api/version` | `last_fetched` and `etag` of the current snapshot, for the auto-update poll |

## Architecture

Background workers scrape Hacker News and GitHub every 8 hours and Reddit about every 2 hours, and store the results in an `ArcSwap`-backed shared state. Every list is pre-rendered and pre-compressed (gzip + zstd) into a fragment when that provider's data changes. `GET /` returns the prebuilt default shell plus the selected lists (from a query or the `tty1_*` view cookies); the client fetches other lists from `/f/...` on first use. Non-default views and custom lists render on demand with a bounded number of blocking render slots; the default request path only selects an encoding and clones prebuilt `Bytes`.

The providers run concurrently and fail independently. Failure is tracked per key — per HN page, per GitHub language and period, per subreddit — so a single dead subreddit keeps its previous posts instead of blanking the panel. A genuinely empty listing is accepted as real data, but an empty Hacker News page or a page that loads with no rows is reported as an error, because that means an API hiccup, block page, or stale selector. There are no retries: a failed fetch fails fast, and the retained snapshot covers it.

The last good snapshot is written to `STATE_DIR/feed.json` after every cycle — queued to a dedicated writer thread, then same-directory temp file, fsync, and `rename`, so a kill never leaves a partial file — and restored before the server binds. A restart therefore paints real panels immediately and `/api/health` answers `200` without waiting for a scrape. The Reddit device identity and anonymous token live beside it in `STATE_DIR/reddit-session.json`, so a restart reuses an unexpired token instead of requesting a new one. Graceful shutdown aborts the workers and flushes the state queue; a hard kill or power loss can lose the newest queued snapshot, but the previous atomically written file remains intact.

The page updates itself. The dashboard bundle polls `/api/version` every minute, on tab focus, and on reconnect, then refetches only the visible or custom list fragments when the snapshot advances. The service worker caches shared `/f/` responses by data version and custom-selection fragments in its browser-local cache. Opened filters stay instant and work offline.

```
Axum server (:3000)
  ├── GET /                  → pre-compressed shell + selected lists from ArcSwap
  ├── GET /f/hn/{page}       → one pre-compressed list fragment
  ├── GET /f/gh/{period}/{language}
  ├── GET /f/reddit/{sub}
  ├── GET /settings          → prebuilt settings page with ETag
  ├── GET /api/health        → 200/503 based on data availability
  └── GET /api/version       → snapshot timestamp + ETag for the client poll

Background tasks (Tokio)
  ├── Scraper (8 h) → HN and GitHub fetch concurrently; each publishes and re-renders as it finishes
  ├── Reddit scraper (~2 h) → random subset via Reddit's OAuth API → merge per key
  └── Shell refresh (1 min) → re-render relative timestamps only
```

## Configuration

All configuration lives in `src/config.rs` as compile-time constants. Key values:

| Constant | Default | Description |
|---|---|---|
| `DEFAULT_PORT` | `3000` | Server port (overridden by `PORT` env var) |
| `SCRAPE_INTERVAL_SECS` | `28800` | Time between Hacker News and GitHub refreshes |
| `REDDIT_SCRAPE_INTERVAL_SECS` | `7200` | Time between Reddit refreshes |
| `HTML_REFRESH_SECS` | `60` | Time between shell re-renders (fragments rebuild on data changes only) |
| `HN_STORIES_PER_PAGE` | `30` | Stories shown per HN page |
| `GITHUB_REPOS_PER_PAGE` | `25` | Repos shown per GitHub language |
| `REDDIT_POSTS_PER_SUB` | `30` | Posts fetched per subreddit |
| `REDDIT_ALL_VIEW_LIMIT` | `100` | Max posts in merged "all" view |
| `CUSTOM_LIST_LIMIT` | `100` | Max items in a private custom list |

## Credits

- Inspired by [hackertab.dev](https://github.com/medyo/hackertab.dev)
- [Simple Icons](https://simpleicons.org/) — Brand icons
- [Lucide](https://lucide.dev/) — Favicon

## License

[MIT](LICENSE)
