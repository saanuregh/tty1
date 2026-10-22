export const KEY = "tty1";
export const DEFAULT_ORDER = ["hn", "gh", "reddit"] as const;
export const SELECTORS = [
	"hn-select",
	"lang-select",
	"subreddit-select",
] as const;

export type Theme = "light" | "dark";
export type SelectorKey = (typeof SELECTORS)[number];

export interface Profile {
	theme?: Theme;
	panels?: string[];
	order?: string[];
	subs?: string[];
	langs?: string[];
	panel?: number;
	gh?: string;
	"hn-select"?: string;
	"lang-select"?: string;
	"subreddit-select"?: string;
}

export function load(): Profile {
	try {
		return JSON.parse(localStorage.getItem(KEY) || "{}") as Profile;
	} catch {
		return {};
	}
}

export function save(profile: Profile): void {
	try {
		localStorage.setItem(KEY, JSON.stringify(profile));
	} catch {}
}

export function $<T extends Element = HTMLElement>(selector: string): T | null {
	return document.querySelector<T>(selector);
}

export function $$<T extends Element = HTMLElement>(
	selector: string,
): NodeListOf<T> {
	return document.querySelectorAll<T>(selector);
}

export function isTyping(event: KeyboardEvent): boolean {
	const target = event.target;
	return (
		target instanceof HTMLElement &&
		(target.tagName === "INPUT" || target.tagName === "TEXTAREA")
	);
}

export function update(fn: (profile: Profile) => void): void {
	const profile = load();
	fn(profile);
	save(profile);
}

export function getTheme(): Theme {
	return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

export function setTheme(theme: Theme): void {
	document.documentElement.dataset.theme = theme;
	document
		.querySelector<HTMLMetaElement>('meta[name="theme-color"]')
		?.setAttribute("content", theme === "light" ? "#f5f5f5" : "#0a0a0a");
	update((profile) => {
		profile.theme = theme;
	});
}

const TIME_UNITS: ReadonlyArray<readonly [number, string]> = [
	[31536000, "y"],
	[2592000, "mo"],
	[604800, "w"],
	[86400, "d"],
	[3600, "h"],
	[60, "m"],
];

export function timeAgo(timestamp: number): string {
	const elapsed = Math.floor(Date.now() / 1000) - timestamp;
	for (const [seconds, label] of TIME_UNITS) {
		const count = Math.floor(elapsed / seconds);
		if (count > 0) return count + label;
	}
	return "0m";
}

export function attrValue(value: unknown): string {
	const text = String(value);
	return typeof CSS !== "undefined" && typeof CSS.escape === "function"
		? CSS.escape(text)
		: text.replace(/["\\]/g, "\\$&");
}

export function refreshTimes(root: Document | Element = document): void {
	for (const element of root.querySelectorAll<HTMLElement>(".time-ago")) {
		const timestamp = Number.parseInt(element.dataset.ts ?? "", 10);
		if (timestamp) element.textContent = timeAgo(timestamp);
	}
	if (root === document) {
		const fetched = $(".last-updated-time");
		if (fetched?.dataset.ts)
			fetched.textContent = timeAgo(Number.parseInt(fetched.dataset.ts, 10));
	}
}

// The server reads these to render the first paint with the selected filters.
// Only the filter keys live here; theme and panel layout stay client-side.
const VIEW_COOKIES = ["hn", "period", "lang", "sub"] as const;
type ViewCookie = (typeof VIEW_COOKIES)[number];

export function clearViewCookies(): void {
	const secure = location.protocol === "https:" ? "; Secure" : "";
	for (const name of VIEW_COOKIES) {
		try {
			document.cookie = `tty1_${name}=; path=/; max-age=0; SameSite=Lax${secure}`;
		} catch {}
	}
}

export function syncViewCookie(): void {
	const profile = load();
	const values: Record<ViewCookie, string | undefined> = {
		hn: profile["hn-select"],
		period: profile.gh ? profile.gh.replace("gh-tab-", "") : undefined,
		lang:
			profile["lang-select"] === "mine" ? undefined : profile["lang-select"],
		sub: profile["subreddit-select"],
	};
	for (const name of VIEW_COOKIES) {
		const value = values[name];
		try {
			const secure = location.protocol === "https:" ? "; Secure" : "";
			if (value) {
				document.cookie = `tty1_${name}=${encodeURIComponent(value)}; path=/; max-age=31536000; SameSite=Lax${secure}`;
			} else {
				document.cookie = `tty1_${name}=; path=/; max-age=0; SameSite=Lax${secure}`;
			}
		} catch {}
	}
}
