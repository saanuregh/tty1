(function () {
	var KEY = "tty1";

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

	// Theme buttons
	function updateThemeBtns() {
		var current = document.documentElement.dataset.theme || "dark";
		document.querySelectorAll(".theme-btn").forEach(function (btn) {
			btn.classList.toggle("active", btn.dataset.theme === current);
		});
	}

	updateThemeBtns();

	document.querySelectorAll(".theme-btn").forEach(function (btn) {
		btn.addEventListener("click", function () {
			var theme = btn.dataset.theme;
			document.documentElement.dataset.theme = theme;
			var d = load();
			d.theme = theme;
			save(d);
			updateThemeBtns();
		});
	});

	// Reset to defaults
	document.querySelector(".reset-btn").addEventListener("click", function () {
		localStorage.removeItem(KEY);
		document.documentElement.removeAttribute("data-theme");
		updateThemeBtns();
	});
})();
