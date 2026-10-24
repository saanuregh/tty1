import {
	$,
	load,
	refreshTimes,
	SELECTORS,
	syncViewCookie,
	update,
} from "./common.ts";
import {
	applyFilters,
	prefetchLanguages,
	prefetchSubreddits,
	prefetchWhenIdle,
} from "./filters.ts";
import { dataVersion, setVersionListener, tellSwVersion } from "./fragments.ts";
import { initKeyboard, resetFocus } from "./keyboard.ts";
import { initPolling, pollVersion } from "./polling.ts";
import { applyProfile, applyUrlProfile } from "./profile.ts";
import { initSwipe } from "./swipe.ts";

setVersionListener(applyFilters);
applyUrlProfile();

const state = load();
for (const selector of SELECTORS) {
	const element = $<HTMLSelectElement>(`.${selector}`);
	const value = state[selector];
	if (
		element &&
		value &&
		Array.from(element.options).some((option) => option.value === value)
	)
		element.value = value;
}
applyProfile();
applyFilters();
refreshTimes();
syncViewCookie();

initKeyboard();

document.addEventListener("change", () => {
	update((profile) => {
		const selected = $<HTMLInputElement>('[name="gh-tab"]:checked');
		if (selected) profile.gh = selected.id;
		for (const selector of SELECTORS) {
			const element = $<HTMLSelectElement>(`.${selector}`);
			if (element) profile[selector] = element.value;
		}
	});
	applyFilters();
	resetFocus();
	syncViewCookie();
});

const languageSelect = $<HTMLSelectElement>(".lang-select");
if (languageSelect) {
	languageSelect.addEventListener("focus", prefetchLanguages);
	languageSelect.addEventListener("pointerdown", prefetchLanguages);
}
const subredditSelect = $<HTMLSelectElement>(".subreddit-select");
if (subredditSelect) {
	subredditSelect.addEventListener("focus", prefetchSubreddits);
	subredditSelect.addEventListener("pointerdown", prefetchSubreddits);
}

// A stale shell served by the service worker should not wait a full poll
// interval before comparing versions.
void pollVersion();
tellSwVersion(dataVersion());
initPolling();
initSwipe();
prefetchWhenIdle();
