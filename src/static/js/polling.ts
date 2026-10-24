import { $, refreshTimes } from "./common.ts";
import { applyFilters } from "./filters.ts";
import { dataVersion, setDataVersion } from "./fragments.ts";
import { resetFocus } from "./keyboard.ts";

const POLL_INTERVAL_MS = 60000;
const POLL_THROTTLE_MS = 5000;
let lastPollAt = 0;

// The dashboard already renders last_fetched into .last-updated-time, so the
// poll compares against the DOM and needs no client-side etag bookkeeping.
export async function pollVersion(): Promise<void> {
	const now = Date.now();
	if (now - lastPollAt < POLL_THROTTLE_MS) return;
	lastPollAt = now;

	const response = await fetch("/api/version", { cache: "no-store" }).catch(
		() => null,
	);
	if (!response?.ok) return;
	const data = (await response.json().catch(() => null)) as {
		last_fetched?: unknown;
	} | null;
	if (!data || typeof data.last_fetched !== "number") return;
	if (data.last_fetched > dataVersion()) refreshData(data.last_fetched);
	// Re-ensures any visible list left behind by a failed refresh; a no-op for
	// lists already at the current version.
	else applyFilters();
}

function refreshData(version: number): void {
	setDataVersion(version);
	// A focused item may belong to a list that is about to be refetched; drop
	// the index defensively.
	resetFocus();
	applyFilters();
	refreshTimes();
	const fetched = $(".last-updated-time");
	if (fetched) {
		fetched.classList.remove("refreshed");
		void fetched.offsetWidth;
		fetched.classList.add("refreshed");
	}
}

const banner = $(".offline-indicator");

function setOffline(offline: boolean): void {
	if (!banner) return;
	banner.classList.toggle("is-hidden", !offline);
	banner.setAttribute("aria-hidden", String(!offline));
	document.body.classList.toggle("is-offline", offline);
}

export function initPolling(): void {
	setInterval(refreshTimes, 60000);
	setInterval(pollVersion, POLL_INTERVAL_MS);
	document.addEventListener("visibilitychange", () => {
		if (document.visibilityState === "visible") void pollVersion();
	});
	window.addEventListener("online", () => {
		void pollVersion();
		applyFilters();
		setOffline(false);
	});
	window.addEventListener("offline", () => setOffline(true));
	setOffline(!navigator.onLine);
}
