(function () {
	var KEY = "tty1";

	// Restore persisted state
	function load() {
		try {
			return JSON.parse(localStorage.getItem(KEY) || "{}");
		} catch (e) {
			return {};
		}
	}
	function save(d) {
		try {
			localStorage.setItem(KEY, JSON.stringify(d));
		} catch (e) {}
	}
	var s = load();
	var r = s.gh && document.getElementById(s.gh);
	if (r) r.checked = true;
	["hn-select", "lang-select", "subreddit-select"].forEach(function (c) {
		var el = document.querySelector("." + c);
		if (el && s[c]) el.value = s[c];
	});

	function applyFilters() {
		// HN: show matching page
		var page = document.querySelector(".hn-select").value;
		document.querySelectorAll(".hn-panel ol.stories").forEach(function (ol) {
			ol.style.display = ol.dataset.forPage === page ? "block" : "none";
		});

		// GitHub tabs: show matching period, highlight active label
		var tabId = document.querySelector('[name="gh-tab"]:checked').id;
		var period = tabId.replace("gh-tab-", "");
		document.querySelectorAll(".tab-content").forEach(function (el) {
			el.classList.toggle("active", el.id === "gh-" + period);
		});
		document.querySelectorAll(".tab-labels label").forEach(function (label) {
			var isActive = label.getAttribute("for") === tabId;
			label.classList.toggle("active", isActive);
			label.setAttribute("aria-selected", isActive);
		});

		// GitHub language: show matching repos in active tab
		var lang = document.querySelector(".lang-select").value;
		document.querySelectorAll(".gh-panel ol.repos").forEach(function (ol) {
			ol.style.display = ol.dataset.forLang === lang ? "block" : "none";
		});

		// Reddit: show matching subreddit list
		var sub = document.querySelector(".subreddit-select").value;
		document
			.querySelectorAll(".reddit-panel ol.reddit-posts")
			.forEach(function (ol) {
				ol.style.display = ol.dataset.forSub === sub ? "block" : "none";
			});
	}

	document.addEventListener("change", function () {
		var d = {},
			r = document.querySelector('[name="gh-tab"]:checked');
		if (r) d.gh = r.id;
		["hn-select", "lang-select", "subreddit-select"].forEach(function (c) {
			var el = document.querySelector("." + c);
			if (el) d[c] = el.value;
		});
		save(d);
		applyFilters();
	});

	applyFilters();

	var banner = document.querySelector(".offline-banner");
	function setOffline(offline) {
		if (!banner) return;
		banner.classList.toggle("is-hidden", !offline);
		document.body.classList.toggle("is-offline", offline);
	}
	setOffline(!navigator.onLine);
	window.addEventListener("online", function () {
		setOffline(false);
	});
	window.addEventListener("offline", function () {
		setOffline(true);
	});

	// Client-side relative time refresh — duplicated from render.rs, keep both in sync.
	var TIME_UNITS = [
		[31536000, "y"],
		[2592000, "mo"],
		[604800, "w"],
		[86400, "d"],
		[3600, "h"],
		[60, "m"],
	];
	function timeAgo(ts) {
		var elapsed = Math.floor(Date.now() / 1000) - ts;
		for (var i = 0; i < TIME_UNITS.length; i++) {
			var count = Math.floor(elapsed / TIME_UNITS[i][0]);
			if (count > 0) return count + TIME_UNITS[i][1];
		}
		return "0m";
	}
	function refreshTimes() {
		document.querySelectorAll(".time-ago").forEach(function (el) {
			var ts = parseInt(el.dataset.ts, 10);
			if (ts) el.textContent = timeAgo(ts);
		});
		var ft = document.querySelector(".last-updated-time");
		if (ft && ft.dataset.ts)
			ft.textContent = timeAgo(parseInt(ft.dataset.ts, 10));
	}
	setInterval(refreshTimes, 60000);

	// Keyboard shortcuts: 1/2/3 focus panels
	document.addEventListener("keydown", function (e) {
		if (
			e.target.tagName === "SELECT" ||
			e.target.tagName === "INPUT" ||
			e.target.tagName === "TEXTAREA"
		)
			return;
		var panels = document.querySelectorAll(".panel");
		var target;
		if (e.key === "1") target = panels[0];
		else if (e.key === "2") target = panels[1];
		else if (e.key === "3") target = panels[2];
		if (target) {
			var el = target.querySelector("a, select, input");
			if (el) el.focus();
		}
	});

	// Expandable repo descriptions
	document.addEventListener("click", function (e) {
		var desc = e.target.closest(".repo-desc");
		if (desc) desc.classList.toggle("expanded");
	});

	// Mobile swipable cards — re-evaluates on viewport change (e.g. rotation)
	var mobileQuery = window.matchMedia("(max-width: 899px)");
	var swipeObserver = null;

	function initSwipe() {
		var dashboard = document.querySelector(".dashboard");
		var panels = document.querySelectorAll(".panel");
		var dots = document.querySelectorAll(".swipe-dot");

		// Restore saved panel
		var savedPanel = parseInt(s.panel || "0", 10);
		dots.forEach(function (d, i) {
			d.classList.toggle("active", i === savedPanel);
		});
		if (savedPanel > 0 && panels[savedPanel]) {
			dashboard.scrollTo({
				left: savedPanel * dashboard.offsetWidth,
				behavior: "instant",
			});
		}

		swipeObserver = new IntersectionObserver(
			function (entries) {
				entries.forEach(function (entry) {
					if (entry.isIntersecting) {
						var idx = Array.prototype.indexOf.call(panels, entry.target);
						dots.forEach(function (d, i) {
							d.classList.toggle("active", i === idx);
						});
						var d = load();
						d.panel = idx;
						save(d);
					}
				});
			},
			{ root: dashboard, threshold: 0.5 },
		);
		panels.forEach(function (p) {
			swipeObserver.observe(p);
		});

		dots.forEach(function (dot, i) {
			dot.addEventListener("click", function () {
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
	mobileQuery.addEventListener("change", function (e) {
		if (e.matches) initSwipe();
		else teardownSwipe();
	});
})();
