const KEY = "tty1";
const DEFAULT_ORDER = ["hn", "gh", "reddit"];

function load() {
	try {
		return JSON.parse(localStorage.getItem(KEY) || "{}");
	} catch (_) {
		return {};
	}
}

function save(d) {
	try {
		localStorage.setItem(KEY, JSON.stringify(d));
	} catch (_) {}
}

function $(sel) {
	return document.querySelector(sel);
}

function $$(sel) {
	return document.querySelectorAll(sel);
}

function isTyping(e) {
	return e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA";
}

// Load profile, apply fn, save back
function update(fn) {
	const d = load();
	fn(d);
	save(d);
}

// Theme

function getTheme() {
	return document.documentElement.dataset.theme || "dark";
}

function setTheme(theme) {
	document.documentElement.dataset.theme = theme;
	update((d) => {
		d.theme = theme;
	});
}

// Time — duplicated from render/utils.rs, keep both in sync.
const TIME_UNITS = [
	[31536000, "y"],
	[2592000, "mo"],
	[604800, "w"],
	[86400, "d"],
	[3600, "h"],
	[60, "m"],
];

function timeAgo(ts) {
	const elapsed = Math.floor(Date.now() / 1000) - ts;
	for (let i = 0; i < TIME_UNITS.length; i++) {
		const count = Math.floor(elapsed / TIME_UNITS[i][0]);
		if (count > 0) return count + TIME_UNITS[i][1];
	}
	return "0m";
}
