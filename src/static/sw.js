// Bump VERSION when you want to force-clear all cached content. Day-to-day
// changes are handled by stale-while-revalidate: the browser serves cached
// content instantly and refreshes in background, so users get new content
// on the next load without a version bump.
const VERSION = "v3";
const CACHE_NAME = `tty1-${VERSION}`;

// "/" is deliberately absent: install runs on the first visit, which may land
// during a cold start, and precaching the loading page would pin it as the
// first paint of every later navigation. "/" is cached on first navigation.
const PRECACHE = ["/manifest.json", "/icon.svg", "/favicon.svg"];

const OFFLINE_FALLBACK =
	'<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">' +
	'<meta name="viewport" content="width=device-width,initial-scale=1">' +
	"<title>tty1 · offline</title><style>" +
	'body{background:#0a0a0a;color:#555;font-family:"JetBrains Mono","Fira Code","SF Mono","Cascadia Code","Consolas",monospace;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;text-align:center}' +
	"</style></head><body><p>offline · no cached content</p></body></html>";

function isNoStore(response) {
	return (response.headers.get("cache-control") || "").includes("no-store");
}

self.addEventListener("install", (event) => {
	event.waitUntil(
		(async () => {
			const cache = await caches.open(CACHE_NAME);
			await cache.addAll(PRECACHE);
			await self.skipWaiting();
		})(),
	);
});

self.addEventListener("activate", (event) => {
	event.waitUntil(
		(async () => {
			const names = await caches.keys();
			await Promise.all(
				names
					.filter((name) => name !== CACHE_NAME)
					.map((name) => caches.delete(name)),
			);
			await self.clients.claim();
		})(),
	);
});

self.addEventListener("fetch", (event) => {
	const url = new URL(event.request.url);
	if (event.request.method !== "GET" || url.origin !== self.location.origin)
		return;

	// Never cache API responses.
	if (url.pathname.startsWith("/api/")) return;

	if (url.pathname === "/") {
		// Only a document navigation gets the cache. A fetch() from app.js must
		// reach the network, or the auto-update poll just reads this cache back.
		if (event.request.mode !== "navigate") return;

		// Stale-while-revalidate: instant load, background refresh.
		event.respondWith(
			(async () => {
				const cache = await caches.open(CACHE_NAME);
				const cached = await cache.match("/");
				const controller = new AbortController();
				const timer = setTimeout(() => controller.abort(), 10000);
				const fresh = fetch(event.request, {
					signal: controller.signal,
				}).then((response) => {
					clearTimeout(timer);
					if (response.ok && !isNoStore(response))
						cache.put("/", response.clone());
					return response;
				});
				return cached || fresh;
			})().catch(async () => {
				const cached = await caches.match("/");
				if (cached) return cached;
				return new Response(OFFLINE_FALLBACK, {
					status: 503,
					headers: { "Content-Type": "text/html; charset=utf-8" },
				});
			}),
		);
		return;
	}

	// Static assets: cache-first
	event.respondWith(
		(async () => {
			const cached = await caches.match(event.request);
			return cached || fetch(event.request);
		})(),
	);
});
