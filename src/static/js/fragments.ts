import { $, load, refreshTimes, timeAgo } from "./common.ts";

export function dataVersion(): number {
	const fetched = $(".last-updated-time");
	return fetched?.dataset.ts ? Number.parseInt(fetched.dataset.ts, 10) : 0;
}

export function tellSwVersion(version: number): void {
	if (!("serviceWorker" in navigator) || !version) return;
	navigator.serviceWorker.ready
		.then((registration) => {
			registration.active?.postMessage({
				type: "fragment-version",
				version: String(version),
			});
		})
		.catch(() => {});
}

export function setDataVersion(version: number): void {
	const fetched = $(".last-updated-time");
	if (!fetched) return;
	fetched.dataset.ts = String(version);
	fetched.textContent = timeAgo(version);
	tellSwVersion(version);
}

let onVersionChange = () => {};

export function setVersionListener(listener: () => void): void {
	onVersionChange = listener;
}

function adoptServerVersion(serverVersion: number): boolean {
	if (serverVersion <= dataVersion()) return false;
	setDataVersion(serverVersion);
	setTimeout(() => onVersionChange(), 0);
	return true;
}

function fragmentUrl(list: HTMLElement, version: number): string | null {
	let base: string | null = null;
	const params = new URLSearchParams({ v: String(version) });
	const profile = load();
	if (list.dataset.forPage !== undefined) {
		base = `/f/hn/${encodeURIComponent(list.dataset.forPage)}`;
	} else if (list.dataset.forLang !== undefined) {
		const tab = list.closest<HTMLElement>(".tab-content");
		const period = tab ? tab.id.replace("gh-", "") : "daily";
		base = `/f/gh/${encodeURIComponent(period)}/${encodeURIComponent(list.dataset.forLang)}`;
		if (list.dataset.forLang === "mine" && profile.langs?.length)
			params.set("langs", profile.langs.join(","));
	} else if (list.dataset.forSub !== undefined) {
		base = `/f/reddit/${encodeURIComponent(list.dataset.forSub)}`;
		if (list.dataset.forSub === "all" && profile.subs?.length)
			params.set("subs", profile.subs.join(","));
	}
	return base ? `${base}?${params}` : null;
}

const listFetches = new WeakMap<HTMLElement, Promise<boolean>>();

function markFailed(list: HTMLElement | null): void {
	if (!list || list.dataset.loaded === "true") return;
	if (list.querySelector(".fragment-error")) return;
	const item = document.createElement("li");
	item.className = "empty-state fragment-error";
	item.textContent = navigator.onLine
		? "could not load"
		: "offline · not cached";
	list.append(item);
}

export function ensureList(
	list: HTMLElement | null,
	retries = 0,
): Promise<boolean> {
	if (!list) return Promise.resolve(true);
	if (!list.isConnected) return Promise.resolve(false);

	const version = dataVersion();
	if (
		list.dataset.loaded === "true" &&
		list.dataset.version === String(version)
	)
		return Promise.resolve(true);

	const pending = listFetches.get(list);
	if (pending) return pending;
	const url = fragmentUrl(list, version);
	if (!url) return Promise.resolve(true);

	const request = (async () => {
		list.dataset.loading = "1";
		let retryIn = -1;
		try {
			const response = await fetch(url).catch(() => null);
			const serverVersion = response
				? Number.parseInt(response.headers.get("x-data-version") || "0", 10) ||
					0
				: 0;
			if (!response?.ok) {
				markFailed(list);
				adoptServerVersion(serverVersion);
				return false;
			}
			const html = await response.text();
			if (!list.isConnected) return false;
			if (version !== dataVersion()) {
				adoptServerVersion(serverVersion);
				retryIn = 0;
				return false;
			}
			if (serverVersion && serverVersion !== version) {
				if (serverVersion > version) {
					adoptServerVersion(serverVersion);
					retryIn = 0;
				} else if (retries < 8) {
					retryIn = 300;
				} else {
					markFailed(list);
				}
				return false;
			}
			const template = document.createElement("template");
			template.innerHTML = html.trim();
			const fresh = template.content.firstElementChild as HTMLElement | null;
			if (!fresh) {
				markFailed(list);
				return false;
			}
			fresh.style.display = list.style.display;
			if (fresh.dataset.version === undefined)
				fresh.dataset.version = String(version);
			list.replaceWith(fresh);
			refreshTimes(fresh);
			return true;
		} finally {
			delete list.dataset.loading;
			listFetches.delete(list);
			if (retryIn >= 0 && list.isConnected)
				setTimeout(
					() => ensureList(list, retryIn ? retries + 1 : retries),
					retryIn,
				);
		}
	})();
	listFetches.set(list, request);
	return request;
}
