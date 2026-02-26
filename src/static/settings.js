(() => {
	function updateThemeBtns() {
		const current = getTheme();
		for (const btn of $$(".theme-btn"))
			btn.classList.toggle("active", btn.dataset.theme === current);
	}

	updateThemeBtns();

	for (const btn of $$(".theme-btn")) {
		btn.addEventListener("click", () => {
			setTheme(btn.dataset.theme);
			updateThemeBtns();
		});
	}

	document.addEventListener("keydown", (e) => {
		if (isTyping(e)) return;
		if (e.key === ",") window.location.href = "/";
	});

	const panelBtns = $$(".panel-toggle");
	const checkGroups = {
		sub: $$(".sub-check"),
		lang: $$(".lang-check"),
	};
	const hnBtns = $$(".hn-btn");
	const periodBtns = $$(".period-btn");
	const shareInput = $(".share-url");
	const shareBtn = $(".share-btn");
	const container = $(".panel-toggles");

	function checked(nodes) {
		return Array.from(nodes)
			.filter((n) => n.checked)
			.map((n) => n.value);
	}
	function activePanels() {
		return Array.from(panelBtns)
			.filter((b) => b.classList.contains("active"))
			.map((b) => b.dataset.panel);
	}
	function panelOrder() {
		return Array.from(container.querySelectorAll(".panel-toggle")).map(
			(b) => b.dataset.panel,
		);
	}
	function activeVal(btns, key) {
		const btn = Array.from(btns).find((b) => b.classList.contains("active"));
		return btn ? btn.dataset[key] : btns[0]?.dataset[key];
	}
	function radioGroup(btns, key, stored, cb) {
		for (const btn of btns) {
			btn.classList.toggle("active", btn.dataset[key] === stored);
			btn.addEventListener("click", () => {
				for (const b of btns) b.classList.remove("active");
				btn.classList.add("active");
				cb();
			});
		}
	}
	function reorderButtons(keys) {
		if (!container) return;
		for (const key of keys) {
			const btn = container.querySelector(`[data-panel="${key}"]`);
			if (btn) container.appendChild(btn);
		}
	}

	function updateToggleText(target) {
		const checks = checkGroups[target];
		const btn = $(`.select-toggle[data-target="${target}"]`);
		if (btn)
			btn.textContent = Array.from(checks).every((c) => c.checked)
				? "deselect all"
				: "select all";
	}

	function generateShareUrl() {
		const params = new URLSearchParams();
		const p = activePanels();
		const s = checked(checkGroups.sub);
		const l = checked(checkGroups.lang);
		if (p.length < panelBtns.length) params.set("panels", p.join(","));
		if (s.length < checkGroups.sub.length) params.set("subs", s.join(","));
		if (l.length < checkGroups.lang.length) params.set("langs", l.join(","));
		const o = panelOrder();
		if (!o.every((k, i) => k === DEFAULT_ORDER[i]))
			params.set("order", o.join(","));
		const hn = activeVal(hnBtns, "hn");
		if (hn !== hnBtns[0]?.dataset.hn) params.set("hn", hn);
		const period = activeVal(periodBtns, "period");
		if (period !== periodBtns[0]?.dataset.period) params.set("period", period);
		const qs = params.toString();
		return `${window.location.origin}/${qs ? `?${qs}` : ""}`;
	}

	function saveProfile() {
		const d = load();
		const sets = [
			["panels", activePanels(), panelBtns.length],
			["subs", checked(checkGroups.sub), checkGroups.sub.length],
			["langs", checked(checkGroups.lang), checkGroups.lang.length],
		];
		for (const [key, values, total] of sets) {
			if (values.length === total) delete d[key];
			else d[key] = values;
		}
		const order = panelOrder();
		if (order.every((k, i) => k === DEFAULT_ORDER[i])) delete d.order;
		else d.order = order;
		const hn = activeVal(hnBtns, "hn");
		if (hn !== hnBtns[0]?.dataset.hn) d["hn-select"] = hn;
		else delete d["hn-select"];
		const period = activeVal(periodBtns, "period");
		if (period !== periodBtns[0]?.dataset.period) d.gh = `gh-tab-${period}`;
		else delete d.gh;
		save(d);
		if (shareInput) shareInput.value = generateShareUrl();
	}

	const d = load();
	if (d.panels)
		for (const b of panelBtns)
			b.classList.toggle("active", d.panels.includes(b.dataset.panel));
	if (d.subs)
		for (const c of checkGroups.sub) c.checked = d.subs.includes(c.value);
	if (d.langs)
		for (const c of checkGroups.lang) c.checked = d.langs.includes(c.value);
	if (d.order) reorderButtons(d.order);
	radioGroup(
		hnBtns,
		"hn",
		d["hn-select"] || hnBtns[0]?.dataset.hn,
		saveProfile,
	);
	radioGroup(
		periodBtns,
		"period",
		d.gh ? d.gh.replace("gh-tab-", "") : periodBtns[0]?.dataset.period,
		saveProfile,
	);
	updateToggleText("sub");
	updateToggleText("lang");
	if (shareInput) shareInput.value = generateShareUrl();

	for (const btn of panelBtns) {
		btn.addEventListener("click", () => {
			btn.classList.toggle("active");
			if (activePanels().length === 0) {
				btn.classList.add("active");
				return;
			}
			saveProfile();
		});
	}

	if (container) {
		for (const btn of panelBtns) {
			btn.draggable = true;
			btn.addEventListener("dragstart", () => btn.classList.add("dragging"));
			btn.addEventListener("dragend", () => {
				btn.classList.remove("dragging");
				saveProfile();
			});
		}
		container.addEventListener("dragover", (e) => {
			e.preventDefault();
			const dragging = container.querySelector(".dragging");
			if (!dragging) return;
			const siblings = Array.from(
				container.querySelectorAll(".panel-toggle:not(.dragging)"),
			);
			const next = siblings.find((s) => {
				const rect = s.getBoundingClientRect();
				return e.clientX < rect.left + rect.width / 2;
			});
			container.insertBefore(dragging, next || null);
		});
	}

	for (const [target, checks] of Object.entries(checkGroups)) {
		for (const cb of checks)
			cb.addEventListener("change", () => {
				updateToggleText(target);
				saveProfile();
			});
	}

	for (const btn of $$(".select-toggle")) {
		btn.addEventListener("click", () => {
			const checks = checkGroups[btn.dataset.target];
			const allChecked = Array.from(checks).every((c) => c.checked);
			for (const c of checks) c.checked = !allChecked;
			updateToggleText(btn.dataset.target);
			saveProfile();
		});
	}

	if (shareBtn) {
		shareBtn.addEventListener("click", () => {
			navigator.clipboard.writeText(shareInput.value).then(() => {
				shareBtn.textContent = "copied!";
				setTimeout(() => {
					shareBtn.textContent = "copy link";
				}, 1500);
			});
		});
	}

	$(".reset-btn").addEventListener("click", () => {
		localStorage.removeItem(KEY);
		document.documentElement.removeAttribute("data-theme");
		updateThemeBtns();
		for (const b of panelBtns) b.classList.add("active");
		for (const [i, b] of hnBtns.entries())
			b.classList.toggle("active", i === 0);
		for (const [i, b] of periodBtns.entries())
			b.classList.toggle("active", i === 0);
		for (const checks of Object.values(checkGroups))
			for (const c of checks) c.checked = true;
		updateToggleText("sub");
		updateToggleText("lang");
		reorderButtons(DEFAULT_ORDER);
		if (shareInput) shareInput.value = generateShareUrl();
	});
})();
