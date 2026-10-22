import {
	$,
	$$,
	attrValue,
	DEFAULT_ORDER,
	load,
	type Profile,
	save,
} from "./common.ts";

const PANELS: ReadonlyArray<readonly [string, string, string]> = [
	["hn", ".hn-panel", ".dot-hn"],
	["gh", ".gh-panel", ".dot-gh"],
	["reddit", ".reddit-panel", ".dot-reddit"],
];

function optionExists(selector: string, value: string): boolean {
	return Boolean(
		document.querySelector(`${selector} option[value="${attrValue(value)}"]`),
	);
}

function hasSelectOption(select: HTMLSelectElement, value: string): boolean {
	return Array.from(select.options).some((option) => option.value === value);
}

function validKeys(raw: string, allowed: readonly string[]): string[] {
	return raw.split(",").filter((key) => allowed.includes(key));
}

function validOptions(
	raw: string,
	selector: string,
	transform: (value: string) => string = (value) => value,
): string[] {
	return raw
		.split(",")
		.filter((value) => optionExists(selector, transform(value)));
}

function parseUrlProfile(): Profile {
	const params = new URLSearchParams(window.location.search);
	const profile: Profile = {};

	const panels = params.get("panels");
	if (panels) {
		const keys = validKeys(panels, DEFAULT_ORDER);
		if (keys.length) profile.panels = keys;
	}

	const order = params.get("order");
	if (order) {
		const keys = validKeys(order, DEFAULT_ORDER);
		if (keys.length) profile.order = keys;
	}

	const subs = params.get("subs");
	if (subs) {
		const values = validOptions(subs, ".subreddit-select");
		if (values.length) profile.subs = values;
	}

	const langs = params.get("langs");
	if (langs) {
		const values = validOptions(langs, ".lang-select", (value) =>
			value.toLowerCase(),
		);
		if (values.length) profile.langs = values;
	}

	const singles = [
		["hn", ".hn-select", "hn-select"],
		["lang", ".lang-select", "lang-select"],
		["sub", ".subreddit-select", "subreddit-select"],
	] as const satisfies ReadonlyArray<
		readonly [string, string, "hn-select" | "lang-select" | "subreddit-select"]
	>;
	for (const [param, selector, key] of singles) {
		const value = params.get(param);
		if (value && optionExists(selector, value)) profile[key] = value;
	}

	const period = params.get("period");
	if (period && document.getElementById(`gh-tab-${period}`))
		profile.gh = `gh-tab-${period}`;

	return profile;
}

export function applyUrlProfile(): void {
	const profile = parseUrlProfile();
	if (Object.keys(profile).length === 0) return;
	save(Object.assign(load(), profile));
	history.replaceState({}, "", window.location.pathname);
}

export function applyProfile(): void {
	const profile = load();
	const { panels, subs, langs, order } = profile;

	const pageSelect = $<HTMLSelectElement>(".hn-select");
	const storedPage = profile["hn-select"];
	if (pageSelect && storedPage && hasSelectOption(pageSelect, storedPage))
		pageSelect.value = storedPage;
	const githubRadio = profile.gh ? document.getElementById(profile.gh) : null;
	if (githubRadio instanceof HTMLInputElement) githubRadio.checked = true;
	else {
		const firstRadio =
			document.querySelector<HTMLInputElement>('[name="gh-tab"]');
		if (firstRadio) firstRadio.checked = true;
	}

	const dashboard = $(".dashboard");
	const panelOrder: readonly string[] = order || DEFAULT_ORDER;
	let visibleCount = 0;
	for (const [key, panelSelector, dotSelector] of PANELS) {
		const show = !panels || panels.includes(key);
		const index = panelOrder.indexOf(key);
		const panel = $(panelSelector);
		const dot = $(dotSelector);
		if (panel) {
			panel.classList.toggle("panel-hidden", !show);
			panel.style.order = String(index);
		}
		if (dot) {
			dot.classList.toggle("dot-hidden", !show);
			dot.style.order = String(index);
		}
		if (show) visibleCount++;
	}
	if (dashboard) dashboard.dataset.visible = String(visibleCount);

	const subredditSelect = $<HTMLSelectElement>(".subreddit-select");
	if (subredditSelect && subs) {
		for (const option of subredditSelect.options) {
			if (option.value !== "all")
				option.style.display = subs.includes(option.value) ? "" : "none";
		}
		const allList = $('.reddit-posts[data-for-sub="all"]');
		if (allList) {
			delete allList.dataset.loaded;
			delete allList.dataset.version;
		}
		if (
			subredditSelect.value !== "all" &&
			!subs.includes(subredditSelect.value)
		) {
			subredditSelect.value = "all";
			subredditSelect.dispatchEvent(new Event("change", { bubbles: true }));
		}
	}

	const languageSelect = $<HTMLSelectElement>(".lang-select");
	if (languageSelect) {
		const hasMine = languageSelect.querySelector<HTMLOptionElement>(
			'option[value="mine"]',
		);
		if (langs?.length) {
			if (!hasMine) {
				const option = document.createElement("option");
				option.value = "mine";
				option.textContent = "mine";
				const firstOption = languageSelect.options[0];
				if (firstOption) firstOption.after(option);
				else languageSelect.append(option);
			}
			const storedLanguage = profile["lang-select"];
			if (storedLanguage && hasSelectOption(languageSelect, storedLanguage))
				languageSelect.value = storedLanguage;
			for (const tab of $$(".tab-content")) {
				tab.querySelector('.repos[data-for-lang="mine"]')?.remove();
				const list = document.createElement("ol");
				list.className = "repos";
				list.dataset.forLang = "mine";
				list.style.display = "none";
				tab.appendChild(list);
			}
		} else {
			if (hasMine) {
				if (languageSelect.value === "mine") {
					languageSelect.value = "all";
					languageSelect.dispatchEvent(new Event("change", { bubbles: true }));
				}
				hasMine.remove();
			}
			for (const list of $$('.repos[data-for-lang="mine"]')) list.remove();
		}
	}
}
