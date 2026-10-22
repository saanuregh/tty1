// Bump VERSION when you want to force-clear all cached content. Day-to-day
// changes are handled by stale-while-revalidate: the browser serves cached
// content instantly and refreshes in background, so users get new content
// on the next load without a version bump.
const VERSION = "v6";
const CACHE_NAME = `tty1-${VERSION}`;

// "/" is deliberately absent: install runs on the first visit, which may land
// during a cold start, and precaching the loading page would pin it as the
// first paint of every later navigation. "/" is cached on first navigation.
const PRECACHE = [
	"/manifest.json",
	"/icon-192.png",
	"/icon-512.png",
	"/apple-touch-icon.png",
	"/favicon.svg",
];

const OFFLINE_FALLBACK =
	'<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">' +
	'<meta name="viewport" content="width=device-width,initial-scale=1">' +
	"<title>tty1 · offline</title><style>" +
	'body{background:#0a0a0a;color:#555;font-family:"JetBrains Mono","Fira Code","SF Mono","Cascadia Code","Consolas",monospace;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;text-align:center}' +
	"</style></head><body><p>offline · no cached content</p></body></html>";

function isNoStore(response) {
	return (response.headers.get("cache-control") || "").includes("no-store");
}

function isPrivate(response) {
	return (response.headers.get("cache-control") || "").includes("private");
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

	// List fragments are versioned in the query string, so cache-first is safe
	// and makes repeat visits and offline filter switches instant.
	if (url.pathname.startsWith("/f/")) {
		event.respondWith(
			(async () => {
				const cache = await caches.open(CACHE_NAME);
				const cached = await cache.match(event.request);
				if (cached) return cached;
				const fresh = await fetch(event.request);
				const requestedVersion = url.searchParams.get("v");
				const isCustom = fresh.headers.get("x-tty1-custom") === "1";
				if (
					fresh.ok &&
					fresh.headers.get("x-data-version") === requestedVersion &&
					(!isPrivate(fresh) || isCustom)
				)
					await cache.put(event.request, fresh.clone());
				return fresh;
			})(),
		);
		return;
	}

	if (url.pathname === "/") {
		// Only a document navigation gets the cache. A fetch() from the dashboard bundle must
		// reach the network, or the auto-update poll just reads this cache back.
		if (event.request.mode !== "navigate") return;

		// Stale-while-revalidate: instant load, background refresh.
		event.respondWith(
			(async () => {
				const cache = await caches.open(CACHE_NAME);
				const cached = await cache.match(event.request);
				const controller = new AbortController();
				const timer = setTimeout(() => controller.abort(), 10000);
				const refresh = fetch(event.request, {
					signal: controller.signal,
				})
					.then(async (response) => {
						if (response.ok && !isNoStore(response))
							await cache.put(event.request, response.clone());
						return response;
					})
					.catch(() => null)
					.finally(() => clearTimeout(timer));

				if (cached) {
					event.waitUntil(refresh);
					return cached;
				}

				const fresh = await refresh;
				if (fresh) return fresh;

				const fallback =
					(await caches.match(event.request)) || (await caches.match("/"));
				if (fallback) return fallback;
				return new Response(OFFLINE_FALLBACK, {
					status: 503,
					headers: { "Content-Type": "text/html; charset=utf-8" },
				});
			})(),
		);
		return;
	}

	// Precache list only. Other paths (notably /settings) must reach the
	// network; caching them cache-first would pin an old page forever.
	if (!PRECACHE.includes(url.pathname)) return;

	event.respondWith(
		(async () => {
			const cache = await caches.open(CACHE_NAME);
			const cached = await cache.match(event.request);
			if (cached) return cached;
			const fresh = await fetch(event.request);
			if (fresh.ok) await cache.put(event.request, fresh.clone());
			return fresh;
		})(),
	);
});

// The page reports the data version so fragments from older snapshots can be
// dropped instead of accumulating across days.
self.addEventListener("message", (event) => {
	const version = event.data?.version;
	if (event.data?.type !== "fragment-version" || !version) return;
	event.waitUntil(
		(async () => {
			const cache = await caches.open(CACHE_NAME);
			const requests = await cache.keys();
			await Promise.all(
				requests.map((request) => {
					const url = new URL(request.url);
					if (
						url.pathname.startsWith("/f/") &&
						url.searchParams.get("v") !== String(version)
					)
						return cache.delete(request);
					return Promise.resolve(false);
				}),
			);
		})(),
	);
});
