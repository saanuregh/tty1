(() => {
	const SELECTORS = ["hn-select", "lang-select", "subreddit-select"];

	function scoreOf(el, sel) {
		return (
			parseInt(el.querySelector(sel)?.textContent.replace(/[^\d]/g, ""), 10) ||
			0
		);
	}

	function mergeClones(listSelector, itemSelector, scoreSel, keys) {
		const items = [];
		for (const k of keys) {
			const ol = $(listSelector.replace("$", k));
			if (ol)
				for (const el of ol.querySelectorAll(itemSelector))
					items.push(el.cloneNode(true));
		}
		items.sort((a, b) => scoreOf(b, scoreSel) - scoreOf(a, scoreSel));
		return items;
	}

	function showList(selector, attr, value) {
		for (const ol of $$(selector))
			ol.style.display = ol.dataset[attr] === value ? "block" : "none";
	}

	function applyFilters() {
		const page = $(".hn-select").value;
		showList(".hn-panel ol.stories", "forPage", page);
		const hnLink = $(".hn-link");
		if (hnLink) {
			const hnPath = page === "top" ? "" : `/${page}`;
			hnLink.href = `https://news.ycombinator.com${hnPath}`;
		}

		const tabId = $('[name="gh-tab"]:checked').id;
		const period = tabId.replace("gh-tab-", "");
		for (const el of $$(".tab-content"))
			el.classList.toggle("active", el.id === `gh-${period}`);
		for (const label of $$(".tab-labels label")) {
			const active = label.getAttribute("for") === tabId;
			label.classList.toggle("active", active);
			label.setAttribute("aria-selected", active);
		}

		const lang = $(".lang-select").value;
		showList(".gh-panel ol.repos", "forLang", lang);
		const ghLink = $(".gh-link");
		if (ghLink) {
			const langPath = lang === "all" || lang === "mine" ? "" : `/${lang}`;
			ghLink.href = `https://github.com/trending${langPath}?since=${period}`;
		}

		const sub = $(".subreddit-select").value;
		showList(".reddit-panel ol.reddit-posts", "forSub", sub);
		const redditLink = $(".reddit-link");
		if (redditLink) {
			redditLink.href =
				sub === "all"
					? "https://www.reddit.com"
					: `https://www.reddit.com/r/${sub}`;
		}
	}

	function applyProfile() {
		const d = load();
		const { panels, subs, langs, order } = d;

		const hnSelect = $(".hn-select");
		if (hnSelect && d["hn-select"]) hnSelect.value = d["hn-select"];
		const ghRadio = d.gh && document.getElementById(d.gh);
		if (ghRadio) ghRadio.checked = true;

		const dashboard = $(".dashboard");
		const PANELS = [
			["hn", ".hn-panel", ".dot-hn"],
			["gh", ".gh-panel", ".dot-gh"],
			["reddit", ".reddit-panel", ".dot-reddit"],
		];
		const panelOrder = order || DEFAULT_ORDER;
		let visibleCount = 0;
		for (const [key, panelSel, dotSel] of PANELS) {
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
		}
		if (dashboard) dashboard.dataset.visible = visibleCount;

		const subSelect = $(".subreddit-select");
		if (subSelect && subs) {
			for (const o of subSelect.options) {
				if (o.value !== "all")
					o.style.display = subs.includes(o.value) ? "" : "none";
			}
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
				for (const tab of $$(".tab-content")) {
					tab.querySelector('.repos[data-for-lang="mine"]')?.remove();
					const ol = document.createElement("ol");
					ol.className = "repos";
					ol.dataset.forLang = "mine";
					ol.style.display = "none";
					const repos = [];
					for (const lang of lowerLangs) {
						const src = tab.querySelector(`.repos[data-for-lang="${lang}"]`);
						if (src)
							for (const r of src.querySelectorAll(".repo"))
								repos.push(r.cloneNode(true));
					}
					repos.sort(
						(a, b) => scoreOf(b, ".repo-stars") - scoreOf(a, ".repo-stars"),
					);
					ol.append(...repos);
					tab.appendChild(ol);
				}
			} else {
				if (hasMine) {
					if (langSelect.value === "mine") {
						langSelect.value = "all";
						langSelect.dispatchEvent(new Event("change", { bubbles: true }));
					}
					hasMine.remove();
				}
				for (const ol of $$('.repos[data-for-lang="mine"]')) ol.remove();
			}
		}
	}

	const params = new URLSearchParams(window.location.search);
	const urlProfile = {};
	for (const k of ["panels", "subs", "langs", "order"]) {
		const v = params.get(k);
		if (v) urlProfile[k] = v.split(",");
	}
	const hnParam = params.get("hn");
	if (hnParam) urlProfile["hn-select"] = hnParam;
	const periodParam = params.get("period");
	if (periodParam) urlProfile.gh = `gh-tab-${periodParam}`;
	if (Object.keys(urlProfile).length) {
		save(Object.assign(load(), urlProfile));
		history.replaceState({}, "", window.location.pathname);
	}

	const state = load();
	for (const c of SELECTORS) {
		const el = $(`.${c}`);
		if (el && state[c]) el.value = state[c];
	}
	applyProfile();
	applyFilters();

	let activePanel = 0;
	let focusedIdx = -1;

	function getVisibleItems(panelIdx) {
		const panel = $$(".panel")[panelIdx];
		if (!panel) return [];
		for (const ol of panel.querySelectorAll("ol")) {
			if (ol.style.display !== "none") return ol.querySelectorAll("li");
		}
		return [];
	}

	function clearFocus() {
		$(".focused")?.classList.remove("focused");
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
		if (newTab)
			a.dispatchEvent(
				new MouseEvent("click", { ctrlKey: true, bubbles: true }),
			);
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

	document.addEventListener("change", () => {
		update((d) => {
			const r = $('[name="gh-tab"]:checked');
			if (r) d.gh = r.id;
			for (const c of SELECTORS) {
				const el = $(`.${c}`);
				if (el) d[c] = el.value;
			}
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
			for (const [i, p] of panels.entries()) {
				if (!p.classList.contains("panel-hidden")) visible.push(i);
			}
			visible.sort(
				(a, b) =>
					(parseInt(panels[a].style.order, 10) || 0) -
					(parseInt(panels[b].style.order, 10) || 0),
			);
			if (visible.length === 0) return;
			const cur = visible.indexOf(activePanel);
			const delta = e.key === "h" ? -1 : 1;
			const next =
				cur === -1 ? 0 : (cur + delta + visible.length) % visible.length;
			activePanel = visible[next];
			clearFocus();
			focusedIdx = -1;
			for (const [i, p] of panels.entries())
				p.classList.toggle("active-panel", i === activePanel);
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

	function refreshTimes() {
		for (const el of $$(".time-ago")) {
			const ts = parseInt(el.dataset.ts, 10);
			if (ts) el.textContent = timeAgo(ts);
		}
		const ft = $(".last-updated-time");
		if (ft?.dataset.ts) ft.textContent = timeAgo(parseInt(ft.dataset.ts, 10));
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

	const banner = $(".offline-banner");
	function setOffline(offline) {
		if (!banner) return;
		banner.classList.toggle("is-hidden", !offline);
		document.body.classList.toggle("is-offline", offline);
	}
	setOffline(!navigator.onLine);
	window.addEventListener("online", () => setOffline(false));
	window.addEventListener("offline", () => setOffline(true));

	const mobileQuery = window.matchMedia("(max-width: 899px)");
	let swipeObserver = null;

	function initSwipe() {
		const dashboard = $(".dashboard");
		const panels = $$(".panel:not(.panel-hidden)");
		const dots = $$(".swipe-dot:not(.dot-hidden)");

		const savedPanel = parseInt(state.panel || "0", 10);
		for (const [i, d] of dots.entries())
			d.classList.toggle("active", i === savedPanel);
		if (savedPanel > 0 && panels[savedPanel]) {
			dashboard.scrollTo({
				left: savedPanel * dashboard.offsetWidth,
				behavior: "instant",
			});
		}

		swipeObserver = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					if (!entry.isIntersecting) continue;
					const idx = Array.prototype.indexOf.call(panels, entry.target);
					for (const [i, d] of dots.entries())
						d.classList.toggle("active", i === idx);
					update((d) => {
						d.panel = idx;
					});
				}
			},
			{ root: dashboard, threshold: 0.5 },
		);
		for (const p of panels) swipeObserver.observe(p);

		for (const [i, dot] of dots.entries()) {
			dot.addEventListener("click", () => {
				panels[i].scrollIntoView({ behavior: "smooth", inline: "start" });
			});
		}
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
