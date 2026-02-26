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

function update(fn) {
	const d = load();
	fn(d);
	save(d);
}

function getTheme() {
	return document.documentElement.dataset.theme || "dark";
}

function setTheme(theme) {
	document.documentElement.dataset.theme = theme;
	update((d) => {
		d.theme = theme;
	});
}

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
	for (const [secs, label] of TIME_UNITS) {
		const count = Math.floor(elapsed / secs);
		if (count > 0) return count + label;
	}
	return "0m";
}
