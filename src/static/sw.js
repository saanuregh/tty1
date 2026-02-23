// Bump VERSION when you want to force-clear all cached content. Day-to-day
// changes are handled by stale-while-revalidate: the browser serves cached
// content instantly and refreshes in background, so users get new content
// on the next load without a version bump.
var VERSION = "v2";
var CACHE_NAME = "tty1-" + VERSION;

var PRECACHE = ["/", "/manifest.json", "/icon.svg", "/favicon.svg"];

var OFFLINE_FALLBACK = [
	"<!DOCTYPE html>",
	'<html lang="en"><head>',
	'<meta charset="utf-8">',
	'<meta name="viewport" content="width=device-width,initial-scale=1">',
	"<title>tty1 · offline</title>",
	"<style>",
	'body{background:#0a0a0a;color:#555;font-family:"JetBrains Mono","Fira Code","SF Mono","Cascadia Code","Consolas",monospace;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;text-align:center}',
	"</style>",
	"</head><body>",
	"<p>offline · no cached content</p>",
	"</body></html>",
].join("");

self.addEventListener("install", function (event) {
	event.waitUntil(
		caches
			.open(CACHE_NAME)
			.then(function (cache) {
				return cache.addAll(PRECACHE);
			})
			.then(function () {
				return self.skipWaiting();
			}),
	);
});

self.addEventListener("activate", function (event) {
	event.waitUntil(
		caches
			.keys()
			.then(function (names) {
				return Promise.all(
					names
						.filter(function (name) {
							return name !== CACHE_NAME;
						})
						.map(function (name) {
							return caches.delete(name);
						}),
				);
			})
			.then(function () {
				return self.clients.claim();
			}),
	);
});

self.addEventListener("fetch", function (event) {
	var url = new URL(event.request.url);
	if (event.request.method !== "GET" || url.origin !== self.location.origin)
		return;

	// Main page: stale-while-revalidate (instant load, background refresh)
	if (url.pathname === "/") {
		event.respondWith(
			caches
				.open(CACHE_NAME)
				.then(function (cache) {
					return cache.match(event.request).then(function (cached) {
						var fresh = Promise.race([
							fetch(event.request),
							new Promise(function (_, reject) {
								setTimeout(function () {
									reject(new Error("timeout"));
								}, 10000);
							}),
						]).then(function (response) {
							if (response.ok) cache.put(event.request, response.clone());
							return response;
						});
						return cached || fresh;
					});
				})
				.catch(function () {
					return caches.match("/").then(function (cached) {
						if (cached) return cached;
						return new Response(OFFLINE_FALLBACK, {
							status: 503,
							headers: { "Content-Type": "text/html; charset=utf-8" },
						});
					});
				}),
		);
		return;
	}

	// Static assets: cache-first
	event.respondWith(
		caches.match(event.request).then(function (cached) {
			if (cached) return cached;
			return fetch(event.request);
		}),
	);
});
