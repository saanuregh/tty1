(function () {
	const SELECTORS = ["hn-select", "lang-select", "subreddit-select"];

	// Helpers

	function scoreOf(el, sel) {
		return (
			parseInt(el.querySelector(sel)?.textContent.replace(/[^\d]/g, "")) || 0
		);
	}

	// Collect cloned items from multiple lists, sorted by score
	function mergeClones(listSelector, itemSelector, scoreSel, keys) {
		const items = [];
		keys.forEach((k) => {
			const ol = $(listSelector.replace("$", k));
			if (ol)
				ol.querySelectorAll(itemSelector).forEach((el) =>
					items.push(el.cloneNode(true)),
				);
		});
		items.sort((a, b) => scoreOf(b, scoreSel) - scoreOf(a, scoreSel));
		return items;
	}

	// Filters

	function applyFilters() {
		const page = $(".hn-select").value;
		$$(".hn-panel ol.stories").forEach((ol) => {
			ol.style.display = ol.dataset.forPage === page ? "block" : "none";
		});

		const tabId = $('[name="gh-tab"]:checked').id;
		const period = tabId.replace("gh-tab-", "");
		$$(".tab-content").forEach((el) =>
			el.classList.toggle("active", el.id === "gh-" + period),
		);
		$$(".tab-labels label").forEach((label) => {
			const active = label.getAttribute("for") === tabId;
			label.classList.toggle("active", active);
			label.setAttribute("aria-selected", active);
		});

		const lang = $(".lang-select").value;
		$$(".gh-panel ol.repos").forEach((ol) => {
			ol.style.display = ol.dataset.forLang === lang ? "block" : "none";
		});

		const sub = $(".subreddit-select").value;
		$$(".reddit-panel ol.reddit-posts").forEach((ol) => {
			ol.style.display = ol.dataset.forSub === sub ? "block" : "none";
		});
	}

	// Profile

	function applyProfile() {
		const d = load();
		const { panels, subs, langs, order } = d;

		// HN/GH defaults
		const hnSelect = $(".hn-select");
		if (hnSelect && d["hn-select"]) hnSelect.value = d["hn-select"];
		const ghRadio = d.gh && document.getElementById(d.gh);
		if (ghRadio) ghRadio.checked = true;

		// Panels — visibility + order
		const dashboard = $(".dashboard");
		const PANELS = [
			["hn", ".hn-panel", ".dot-hn"],
			["gh", ".gh-panel", ".dot-gh"],
			["reddit", ".reddit-panel", ".dot-reddit"],
		];
		const panelOrder = order || DEFAULT_ORDER;
		let visibleCount = 0;
		PANELS.forEach(([key, panelSel, dotSel]) => {
			const show = !panels || panels.includes(key);
			const idx = panelOrder.indexOf(key);
			const panel = $(panelSel);
			const dot = $(dotSel);
			if (panel) {
				panel.classList.toggle("panel-hidden", !show);
				panel.style.order = idx;
			}
			if (dot) {
				dot.classList.toggle("dot-hidden", !show);
				dot.style.order = idx;
			}
			if (show) visibleCount++;
		});
		if (dashboard) dashboard.dataset.visible = visibleCount;

		// Reddit subs — filter dropdown + rebuild "all" list
		const subSelect = $(".subreddit-select");
		if (subSelect && subs) {
			Array.from(subSelect.options).forEach((o) => {
				if (o.value !== "all")
					o.style.display = subs.includes(o.value) ? "" : "none";
			});
			if (subSelect.value !== "all" && !subs.includes(subSelect.value)) {
				subSelect.value = "all";
				subSelect.dispatchEvent(new Event("change", { bubbles: true }));
			}
			const allList = $('.reddit-posts[data-for-sub="all"]');
			if (allList) {
				allList.replaceChildren(
					...mergeClones(
						'.reddit-posts[data-for-sub="$"]',
						".reddit-post",
						".reddit-score",
						subs,
					),
				);
			}
		}

		// GitHub "mine" filter
		const langSelect = $(".lang-select");
		if (langSelect) {
			const hasMine = langSelect.querySelector('option[value="mine"]');
			if (langs) {
				if (!hasMine) {
					const opt = document.createElement("option");
					opt.value = "mine";
					opt.textContent = "mine";
					langSelect.options[0].after(opt);
				}
				const lowerLangs = langs.map((l) => l.toLowerCase());
				$$(".tab-content").forEach((tab) => {
					const old = tab.querySelector('.repos[data-for-lang="mine"]');
					if (old) old.remove();
					const ol = document.createElement("ol");
					ol.className = "repos";
					ol.dataset.forLang = "mine";
					ol.style.display = "none";
					const repos = [];
					lowerLangs.forEach((lang) => {
						const src = tab.querySelector(
							'.repos[data-for-lang="' + lang + '"]',
						);
						if (src)
							src
								.querySelectorAll(".repo")
								.forEach((r) => repos.push(r.cloneNode(true)));
					});
					repos.sort(
						(a, b) => scoreOf(b, ".repo-stars") - scoreOf(a, ".repo-stars"),
					);
					ol.append(...repos);
					tab.appendChild(ol);
				});
			} else {
				if (hasMine) {
					if (langSelect.value === "mine") {
						langSelect.value = "all";
						langSelect.dispatchEvent(new Event("change", { bubbles: true }));
					}
					hasMine.remove();
				}
				$$('.repos[data-for-lang="mine"]').forEach((ol) => ol.remove());
			}
		}
	}

	// Import profile from URL params
	const params = new URLSearchParams(window.location.search);
	const urlProfile = {};
	["panels", "subs", "langs", "order"].forEach((k) => {
		const v = params.get(k);
		if (v) urlProfile[k] = v.split(",");
	});
	const hnParam = params.get("hn");
	if (hnParam) urlProfile["hn-select"] = hnParam;
	const periodParam = params.get("period");
	if (periodParam) urlProfile.gh = "gh-tab-" + periodParam;
	if (Object.keys(urlProfile).length) {
		save(Object.assign(load(), urlProfile));
		history.replaceState({}, "", window.location.pathname);
	}

	const state = load();
	SELECTORS.forEach((c) => {
		const el = $("." + c);
		if (el && state[c]) el.value = state[c];
	});
	applyProfile();
	applyFilters();

	// Keyboard navigation

	let activePanel = 0;
	let focusedIdx = -1;

	function getVisibleItems(panelIdx) {
		const panel = $$(".panel")[panelIdx];
		if (!panel) return [];
		const lists = panel.querySelectorAll("ol");
		for (let i = 0; i < lists.length; i++) {
			if (lists[i].style.display !== "none")
				return lists[i].querySelectorAll("li");
		}
		return [];
	}

	function clearFocus() {
		const el = $(".focused");
		if (el) el.classList.remove("focused");
	}

	function setFocus(idx) {
		clearFocus();
		const items = getVisibleItems(activePanel);
		if (idx < 0 || idx >= items.length) return;
		focusedIdx = idx;
		items[idx].classList.add("focused");
		items[idx].scrollIntoView({ block: "nearest" });
	}

	function getFocusedItem() {
		const items = getVisibleItems(activePanel);
		return focusedIdx >= 0 && focusedIdx < items.length
			? items[focusedIdx]
			: null;
	}

	function navigate(a, newTab) {
		if (!a) return;
		if (newTab) window.open(a.href, "_blank");
		else window.location.href = a.href;
	}

	function openFocusedLink(newTab) {
		navigate(getFocusedItem()?.querySelector("a"), newTab);
	}

	function openFocusedComments(newTab) {
		const item = getFocusedItem();
		if (!item) return;
		const links = item.querySelectorAll("a");
		navigate(links.length > 1 ? links[links.length - 1] : links[0], newTab);
	}

	// Event listeners

	document.addEventListener("change", () => {
		update((d) => {
			const r = $('[name="gh-tab"]:checked');
			if (r) d.gh = r.id;
			SELECTORS.forEach((c) => {
				const el = $("." + c);
				if (el) d[c] = el.value;
			});
		});
		applyFilters();
		clearFocus();
		focusedIdx = -1;
	});

	document.addEventListener("keydown", (e) => {
		if (e.key === "Escape" && document.activeElement) {
			document.activeElement.blur();
			return;
		}
		if (e.target.tagName === "SELECT") {
			if (e.key === "j" || e.key === "k") {
				e.preventDefault();
				const sel = e.target;
				const i = sel.selectedIndex + (e.key === "j" ? 1 : -1);
				if (i >= 0 && i < sel.options.length) {
					sel.selectedIndex = i;
					sel.dispatchEvent(new Event("change", { bubbles: true }));
				}
				return;
			}
			if (e.key === "h" || e.key === "l") e.target.blur();
			else return;
		}
		if (isTyping(e)) return;

		if (e.key === "h" || e.key === "l") {
			e.preventDefault();
			const panels = $$(".panel");
			const visible = [];
			panels.forEach((p, i) => {
				if (!p.classList.contains("panel-hidden")) visible.push(i);
			});
			visible.sort(
				(a, b) =>
					(parseInt(panels[a].style.order) || 0) -
					(parseInt(panels[b].style.order) || 0),
			);
			if (visible.length === 0) return;
			const cur = visible.indexOf(activePanel);
			let next;
			if (cur === -1) next = 0;
			else if (e.key === "h")
				next = (cur - 1 + visible.length) % visible.length;
			else next = (cur + 1) % visible.length;
			activePanel = visible[next];
			clearFocus();
			focusedIdx = -1;
			panels.forEach((p, i) =>
				p.classList.toggle("active-panel", i === activePanel),
			);
		} else if (e.key === "f") {
			e.preventDefault();
			const panel = $$(".panel")[activePanel];
			if (!panel) return;
			const controls = panel.querySelectorAll("select, .tab-labels label");
			if (controls.length === 0) return;
			const cur = Array.prototype.indexOf.call(
				controls,
				document.activeElement,
			);
			controls[(cur + 1) % controls.length].focus();
		} else if (e.key === "j") {
			e.preventDefault();
			const items = getVisibleItems(activePanel);
			if (items.length > 0)
				setFocus(Math.min(focusedIdx + 1, items.length - 1));
		} else if (e.key === "k") {
			e.preventDefault();
			if (focusedIdx > 0) setFocus(focusedIdx - 1);
		} else if (e.key === "Enter" && focusedIdx >= 0) {
			e.preventDefault();
			openFocusedLink(e.shiftKey);
		} else if (e.key === "c" && focusedIdx >= 0) {
			e.preventDefault();
			openFocusedComments(e.shiftKey);
		} else if (e.key === "x" && focusedIdx >= 0) {
			e.preventDefault();
			const desc = getFocusedItem()?.querySelector(".repo-desc");
			if (desc) desc.classList.toggle("expanded");
		} else if (e.key === "r") {
			e.preventDefault();
			window.location.reload();
		} else if (e.key === "t") {
			setTheme(getTheme() === "dark" ? "light" : "dark");
		} else if (e.key === ",") {
			window.location.href = "/settings";
		}
	});

	document.addEventListener("click", (e) => {
		const desc = e.target.closest(".repo-desc");
		if (desc) desc.classList.toggle("expanded");
	});

	// Time refresh + auto-refresh

	function refreshTimes() {
		$$(".time-ago").forEach((el) => {
			const ts = parseInt(el.dataset.ts, 10);
			if (ts) el.textContent = timeAgo(ts);
		});
		const ft = $(".last-updated-time");
		if (ft && ft.dataset.ts)
			ft.textContent = timeAgo(parseInt(ft.dataset.ts, 10));
	}

	let etag = "";
	async function autoRefresh() {
		const headers = {};
		if (etag) headers["If-None-Match"] = etag;
		const res = await fetch("/", { headers }).catch(() => null);
		if (!res || res.status === 304) return;
		etag = res.headers.get("etag") || "";
		const html = await res.text();
		const doc = new DOMParser().parseFromString(html, "text/html");
		const fresh = doc.querySelector(".dashboard");
		const current = $(".dashboard");
		if (!fresh || !current) return;
		current.replaceChildren(...fresh.childNodes);
		applyProfile();
		applyFilters();
		refreshTimes();
		const ft = $(".last-updated-time");
		if (ft) {
			ft.classList.remove("refreshed");
			void ft.offsetWidth;
			ft.classList.add("refreshed");
		}
		if (mobileQuery.matches) {
			teardownSwipe();
			initSwipe();
		}
	}

	setInterval(refreshTimes, 60000);
	setInterval(autoRefresh, 900000);

	// Offline banner
	const banner = $(".offline-banner");
	function setOffline(offline) {
		if (!banner) return;
		banner.classList.toggle("is-hidden", !offline);
		document.body.classList.toggle("is-offline", offline);
	}
	setOffline(!navigator.onLine);
	window.addEventListener("online", () => setOffline(false));
	window.addEventListener("offline", () => setOffline(true));

	// Mobile swipe
	const mobileQuery = window.matchMedia("(max-width: 899px)");
	let swipeObserver = null;

	function initSwipe() {
		const dashboard = $(".dashboard");
		const panels = $$(".panel:not(.panel-hidden)");
		const dots = $$(".swipe-dot:not(.dot-hidden)");

		const savedPanel = parseInt(state.panel || "0", 10);
		dots.forEach((d, i) => d.classList.toggle("active", i === savedPanel));
		if (savedPanel > 0 && panels[savedPanel]) {
			dashboard.scrollTo({
				left: savedPanel * dashboard.offsetWidth,
				behavior: "instant",
			});
		}

		swipeObserver = new IntersectionObserver(
			(entries) => {
				entries.forEach((entry) => {
					if (!entry.isIntersecting) return;
					const idx = Array.prototype.indexOf.call(panels, entry.target);
					dots.forEach((d, i) => d.classList.toggle("active", i === idx));
					update((d) => {
						d.panel = idx;
					});
				});
			},
			{ root: dashboard, threshold: 0.5 },
		);
		panels.forEach((p) => swipeObserver.observe(p));

		dots.forEach((dot, i) => {
			dot.addEventListener("click", () => {
				panels[i].scrollIntoView({ behavior: "smooth", inline: "start" });
			});
		});
	}

	function teardownSwipe() {
		if (swipeObserver) {
			swipeObserver.disconnect();
			swipeObserver = null;
		}
	}

	if (mobileQuery.matches) initSwipe();
	mobileQuery.addEventListener("change", (e) => {
		if (e.matches) initSwipe();
		else teardownSwipe();
	});
})();
