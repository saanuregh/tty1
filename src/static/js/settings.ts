import {
	$,
	$$,
	clearViewCookies,
	DEFAULT_ORDER,
	getTheme,
	isTyping,
	KEY,
	load,
	save,
	setTheme,
	syncViewCookie,
} from "./common.ts";

(() => {
	function updateThemeBtns(): void {
		const current = getTheme();
		for (const button of $$<HTMLButtonElement>(".theme-btn")) {
			const active = button.dataset.theme === current;
			button.classList.toggle("active", active);
			button.setAttribute("aria-pressed", String(active));
		}
	}

	updateThemeBtns();

	for (const button of $$<HTMLButtonElement>(".theme-btn")) {
		button.addEventListener("click", () => {
			const theme = button.dataset.theme;
			if (theme === "light" || theme === "dark") setTheme(theme);
			updateThemeBtns();
		});
	}

	document.addEventListener("keydown", (event) => {
		if (isTyping(event)) return;
		if (event.key === ",") window.location.href = "/";
	});

	const panelBtns = $$<HTMLButtonElement>(".panel-toggle");
	const checkGroups = {
		sub: $$<HTMLInputElement>(".sub-check"),
		lang: $$<HTMLInputElement>(".lang-check"),
	} as const;
	type CheckTarget = keyof typeof checkGroups;
	const hnBtns = $$<HTMLButtonElement>(".hn-btn");
	const periodBtns = $$<HTMLButtonElement>(".period-btn");
	const shareInput = $<HTMLInputElement>(".share-url");
	const shareBtn = $<HTMLButtonElement>(".share-btn");
	const container = $(".panel-toggles");

	function checked(nodes: Iterable<HTMLInputElement>): string[] {
		return Array.from(nodes)
			.filter((node) => node.checked)
			.map((node) => node.value);
	}

	function activePanels(): string[] {
		return Array.from(panelBtns)
			.filter((button) => button.classList.contains("active"))
			.flatMap((button) =>
				button.dataset.panel ? [button.dataset.panel] : [],
			);
	}

	function panelOrder(): string[] {
		if (!container) return [];
		return Array.from(
			container.querySelectorAll<HTMLButtonElement>(".panel-toggle"),
		).flatMap((button) => (button.dataset.panel ? [button.dataset.panel] : []));
	}

	function activeVal(
		buttons: NodeListOf<HTMLButtonElement>,
		key: string,
	): string | undefined {
		const button = Array.from(buttons).find((candidate) =>
			candidate.classList.contains("active"),
		);
		return button ? button.dataset[key] : buttons[0]?.dataset[key];
	}

	function updatePanelState(): void {
		for (const button of panelBtns) {
			button.setAttribute(
				"aria-pressed",
				String(button.classList.contains("active")),
			);
		}
	}

	function radioGroup(
		buttons: NodeListOf<HTMLButtonElement>,
		key: string,
		stored: string | undefined,
		callback: () => void,
	): void {
		for (const button of buttons) {
			const active = button.dataset[key] === stored;
			button.classList.toggle("active", active);
			button.setAttribute("aria-pressed", String(active));
			button.addEventListener("click", () => {
				for (const candidate of buttons) {
					candidate.classList.remove("active");
					candidate.setAttribute("aria-pressed", "false");
				}
				button.classList.add("active");
				button.setAttribute("aria-pressed", "true");
				callback();
			});
		}
	}

	function reorderButtons(keys: readonly string[]): void {
		if (!container) return;
		for (const key of keys) {
			const button = container.querySelector<HTMLElement>(
				`[data-panel="${key}"]`,
			);
			if (button) container.appendChild(button);
		}
	}

	function updateToggleText(target: CheckTarget): void {
		const checks = checkGroups[target];
		const button = $<HTMLButtonElement>(
			`.select-toggle[data-target="${target}"]`,
		);
		if (button)
			button.textContent = Array.from(checks).every((check) => check.checked)
				? "deselect all"
				: "select all";
	}

	function currentProfile() {
		return {
			panels: activePanels(),
			subs: checked(checkGroups.sub),
			langs: checked(checkGroups.lang),
			order: panelOrder(),
			hn: activeVal(hnBtns, "hn"),
			period: activeVal(periodBtns, "period"),
		};
	}

	function generateShareUrl(): string {
		const params = new URLSearchParams();
		const profile = currentProfile();
		if (profile.panels.length < panelBtns.length)
			params.set("panels", profile.panels.join(","));
		if (profile.subs.length < checkGroups.sub.length)
			params.set("subs", profile.subs.join(","));
		if (profile.langs.length < checkGroups.lang.length)
			params.set("langs", profile.langs.join(","));
		if (!profile.order.every((key, index) => key === DEFAULT_ORDER[index]))
			params.set("order", profile.order.join(","));
		if (profile.hn && profile.hn !== hnBtns[0]?.dataset.hn)
			params.set("hn", profile.hn);
		if (profile.period && profile.period !== periodBtns[0]?.dataset.period)
			params.set("period", profile.period);
		const query = params.toString();
		return `${window.location.origin}/${query ? `?${query}` : ""}`;
	}

	function saveProfile(): void {
		const profile = load();
		const current = currentProfile();
		const sets: ReadonlyArray<
			readonly ["panels" | "subs" | "langs", string[], number]
		> = [
			["panels", current.panels, panelBtns.length],
			["subs", current.subs, checkGroups.sub.length],
			["langs", current.langs, checkGroups.lang.length],
		];
		for (const [key, values, total] of sets) {
			if (values.length === total) delete profile[key];
			else profile[key] = values;
		}
		if (current.order.every((key, index) => key === DEFAULT_ORDER[index]))
			delete profile.order;
		else profile.order = current.order;
		if (current.hn && current.hn !== hnBtns[0]?.dataset.hn)
			profile["hn-select"] = current.hn;
		else delete profile["hn-select"];
		if (current.period && current.period !== periodBtns[0]?.dataset.period)
			profile.gh = `gh-tab-${current.period}`;
		else delete profile.gh;
		save(profile);
		syncViewCookie();
		if (shareInput) shareInput.value = generateShareUrl();
	}

	const profile = load();
	if (profile.panels) {
		const panels = profile.panels;
		for (const button of panelBtns) {
			const panel = button.dataset.panel;
			if (panel) button.classList.toggle("active", panels.includes(panel));
		}
	}
	if (profile.subs) {
		const subs = profile.subs;
		for (const check of checkGroups.sub)
			check.checked = subs.includes(check.value);
	}
	if (profile.langs) {
		const langs = profile.langs;
		for (const check of checkGroups.lang)
			check.checked = langs.includes(check.value);
	}
	if (profile.order) reorderButtons(profile.order);
	updatePanelState();
	radioGroup(
		hnBtns,
		"hn",
		profile["hn-select"] || hnBtns[0]?.dataset.hn,
		saveProfile,
	);
	radioGroup(
		periodBtns,
		"period",
		profile.gh
			? profile.gh.replace("gh-tab-", "")
			: periodBtns[0]?.dataset.period,
		saveProfile,
	);
	updateToggleText("sub");
	updateToggleText("lang");
	syncViewCookie();
	if (shareInput) shareInput.value = generateShareUrl();

	for (const button of panelBtns) {
		button.addEventListener("click", () => {
			button.classList.toggle("active");
			if (activePanels().length === 0) {
				button.classList.add("active");
				updatePanelState();
				return;
			}
			updatePanelState();
			saveProfile();
		});
	}

	if (container) {
		function movePanel(button: HTMLButtonElement, delta: number): void {
			const siblings = Array.from(
				container?.querySelectorAll<HTMLButtonElement>(".panel-toggle") ?? [],
			);
			const target = siblings[siblings.indexOf(button) + delta];
			if (!target) return;
			if (delta < 0) container?.insertBefore(button, target);
			else container?.insertBefore(button, target.nextSibling);
			button.focus();
			saveProfile();
		}

		container.addEventListener("keydown", (event) => {
			if (!(event.target instanceof Element)) return;
			const button = event.target.closest<HTMLButtonElement>(".panel-toggle");
			if (!button) return;
			if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
				event.preventDefault();
				movePanel(button, -1);
			} else if (event.key === "ArrowDown" || event.key === "ArrowRight") {
				event.preventDefault();
				movePanel(button, 1);
			}
		});

		for (const button of panelBtns) {
			button.draggable = true;
			button.addEventListener("dragstart", () =>
				button.classList.add("dragging"),
			);
			button.addEventListener("dragend", () => {
				button.classList.remove("dragging");
				saveProfile();
			});
		}
		container.addEventListener("dragover", (event) => {
			event.preventDefault();
			const dragging = container.querySelector<HTMLElement>(".dragging");
			if (!dragging) return;
			const siblings = Array.from(
				container.querySelectorAll<HTMLElement>(".panel-toggle:not(.dragging)"),
			);
			const next = siblings.find((sibling) => {
				const rect = sibling.getBoundingClientRect();
				return event.clientX < rect.left + rect.width / 2;
			});
			container.insertBefore(dragging, next || null);
		});
	}

	for (const target of ["sub", "lang"] as const) {
		for (const check of checkGroups[target])
			check.addEventListener("change", () => {
				updateToggleText(target);
				saveProfile();
			});
	}

	for (const button of $$<HTMLButtonElement>(".select-toggle")) {
		button.addEventListener("click", () => {
			const target = button.dataset.target;
			if (target !== "sub" && target !== "lang") return;
			const checks = checkGroups[target];
			const allChecked = Array.from(checks).every((check) => check.checked);
			for (const check of checks) check.checked = !allChecked;
			updateToggleText(target);
			saveProfile();
		});
	}

	if (shareBtn && shareInput) {
		shareBtn.addEventListener("click", () => {
			if (!navigator.clipboard) {
				shareBtn.textContent = "copy unavailable";
				setTimeout(() => {
					shareBtn.textContent = "copy link";
				}, 1500);
				return;
			}
			navigator.clipboard
				.writeText(shareInput.value)
				.then(() => {
					shareBtn.textContent = "copied!";
				})
				.catch(() => {
					shareBtn.textContent = "copy failed";
				})
				.finally(() => {
					setTimeout(() => {
						shareBtn.textContent = "copy link";
					}, 1500);
				});
		});
	}

	const resetButton = $<HTMLButtonElement>(".reset-btn");
	resetButton?.addEventListener("click", () => {
		localStorage.removeItem(KEY);
		clearViewCookies();
		document.documentElement.removeAttribute("data-theme");
		document
			.querySelector<HTMLMetaElement>('meta[name="theme-color"]')
			?.setAttribute("content", "#0a0a0a");
		updateThemeBtns();
		for (const button of panelBtns) button.classList.add("active");
		updatePanelState();
		for (const [index, button] of hnBtns.entries()) {
			const active = index === 0;
			button.classList.toggle("active", active);
			button.setAttribute("aria-pressed", String(active));
		}
		for (const [index, button] of periodBtns.entries()) {
			const active = index === 0;
			button.classList.toggle("active", active);
			button.setAttribute("aria-pressed", String(active));
		}
		for (const checks of Object.values(checkGroups))
			for (const check of checks) check.checked = true;
		updateToggleText("sub");
		updateToggleText("lang");
		reorderButtons(DEFAULT_ORDER);
		if (shareInput) shareInput.value = generateShareUrl();
	});
})();
