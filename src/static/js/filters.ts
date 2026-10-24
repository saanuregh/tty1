import { $, $$, attrValue } from "./common.ts";
import { ensureList } from "./fragments.ts";

export function panelVisible(list: Element): boolean {
	return !list.closest(".panel")?.classList.contains("panel-hidden");
}

function showList(selector: string, attribute: string, value: string): void {
	for (const list of $$(selector)) {
		const match = list.dataset[attribute] === value;
		list.style.display = match ? "block" : "none";
		if (match && panelVisible(list)) ensureList(list);
	}
}

export function prefetchLanguages(): void {
	const languageSelect = $<HTMLSelectElement>(".lang-select");
	const tab = $(".tab-content.active");
	if (!languageSelect || !tab || !panelVisible(tab)) return;
	for (const option of languageSelect.options) {
		if (option.value === "all" || option.value === "mine") continue;
		if (option.style.display === "none") continue;
		const list = tab.querySelector<HTMLElement>(
			`.repos[data-for-lang="${option.value}"]`,
		);
		if (list) ensureList(list);
	}
}

export function prefetchSubreddits(): void {
	const subredditSelect = $<HTMLSelectElement>(".subreddit-select");
	if (!subredditSelect) return;
	for (const option of subredditSelect.options) {
		if (option.value === "all" || option.style.display === "none") continue;
		const list = $<HTMLElement>(
			`.reddit-posts[data-for-sub="${attrValue(option.value)}"]`,
		);
		if (list && panelVisible(list)) ensureList(list);
	}
}

// Loads the other HN pages so a page switch is served from the DOM instead of
// showing skeleton rows. GitHub needs no prefetch: applyFilters already loads
// the current language in every period tab.
export function prefetchWhenIdle(): void {
	const run = () => {
		for (const list of $$(".hn-panel ol.stories")) {
			if (list.style.display === "none" && panelVisible(list)) ensureList(list);
		}
	};
	if ("requestIdleCallback" in window) requestIdleCallback(run);
	else setTimeout(run, 1000);
}

export function applyFilters(): void {
	const pageSelect = $<HTMLSelectElement>(".hn-select");
	if (!pageSelect) return;
	const page = pageSelect.value;
	showList(".hn-panel ol.stories", "forPage", page);
	const hackerNewsLink = $<HTMLAnchorElement>(".hn-link");
	if (hackerNewsLink) {
		const path = page === "top" ? "" : `/${page}`;
		hackerNewsLink.href = `https://news.ycombinator.com${path}`;
	}

	const checkedTab = $<HTMLInputElement>('[name="gh-tab"]:checked');
	if (!checkedTab) return;
	const tabId = checkedTab.id;
	const period = tabId.replace("gh-tab-", "");
	for (const tab of $$(".tab-content"))
		tab.classList.toggle("active", tab.id === `gh-${period}`);
	for (const label of $$<HTMLLabelElement>(".tab-labels label")) {
		const active = label.getAttribute("for") === tabId;
		label.classList.toggle("active", active);
	}

	const languageSelect = $<HTMLSelectElement>(".lang-select");
	if (languageSelect) {
		const language = languageSelect.value;
		showList(".gh-panel ol.repos", "forLang", language);
		const githubLink = $<HTMLAnchorElement>(".gh-link");
		if (githubLink) {
			const path =
				language === "all" || language === "mine" ? "" : `/${language}`;
			githubLink.href = `https://github.com/trending${path}?since=${period}`;
		}
	}

	const subredditSelect = $<HTMLSelectElement>(".subreddit-select");
	if (!subredditSelect) return;
	const subreddit = subredditSelect.value;
	showList(".reddit-panel ol.reddit-posts", "forSub", subreddit);
	const redditLink = $<HTMLAnchorElement>(".reddit-link");
	if (redditLink) {
		redditLink.href =
			subreddit === "all"
				? "https://www.reddit.com"
				: `https://www.reddit.com/r/${subreddit}`;
	}
}
