import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { cp, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { AxeBuilder } from "@axe-core/playwright";
import { chromium } from "playwright";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const binary = path.join(root, "target", "debug", "tty1");
const port = 3987;
const base = `http://127.0.0.1:${port}`;
async function assertAccessible(page, label) {
	const results = await new AxeBuilder({ page }).analyze();
	const violations = results.violations.map((violation) => ({
		id: violation.id,
		impact: violation.impact,
		targets: violation.nodes.map((node) => node.target),
	}));
	assert.deepEqual(violations, [], `${label} accessibility violations`);
}

const stateDir = await mkdtemp(path.join(tmpdir(), "tty1-browser-"));
await cp(path.join(root, "tests", "fixtures", "state"), stateDir, {
	recursive: true,
});

const server = spawn(binary, [], {
	env: {
		...process.env,
		PORT: String(port),
		STATE_DIR: stateDir,
		DISABLE_SCRAPERS: "1",
		DISABLE_REDDIT: "1",
		RUST_LOG: "error",
	},
	stdio: ["ignore", "pipe", "pipe"],
});
let serverError = "";
server.stderr.on("data", (chunk) => {
	serverError += chunk.toString();
});

async function waitForHealth() {
	const deadline = Date.now() + 15000;
	while (Date.now() < deadline) {
		if (server.exitCode !== null) {
			throw new Error(`server exited early:\n${serverError}`);
		}
		try {
			const response = await fetch(`${base}/api/health`);
			if (response.status === 200) return;
		} catch {}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	throw new Error(`server did not become healthy:\n${serverError}`);
}

let browser;
try {
	await waitForHealth();
	browser = await chromium.launch({
		headless: true,
		executablePath: process.env.CHROME_PATH || undefined,
	});
	const context = await browser.newContext({
		viewport: { width: 1280, height: 900 },
	});
	const page = await context.newPage();
	const errors = [];
	page.on("pageerror", (error) => errors.push(error.message));
	page.on("console", (message) => {
		if (message.type() === "error") errors.push(message.text());
	});

	await page.goto(base, { waitUntil: "networkidle" });
	await page.waitForSelector(".dashboard");
	assert.equal(await page.title(), "tty1");
	assert.equal(
		await page.locator(".dashboard").getAttribute("data-visible"),
		"3",
	);

	// Only the selected HN page and selected GitHub period are in the raw
	// first paint; the browser may prefetch the other periods after hydration.
	const raw = await (await fetch(base)).text();
	assert.match(raw, /data-for-page="top" data-loaded="true"/);
	assert.doesNotMatch(raw, /data-for-page="newest" data-loaded/);
	assert.equal(
		(raw.match(/data-for-lang="all" data-loaded="true"/g) || []).length,
		1,
	);

	assert.equal(
		await page.evaluate(async () => {
			const response = await fetch("/icon-192.png");
			return response.status;
		}),
		200,
	);
	assert.equal(
		await page.evaluate(async () => {
			const response = await fetch("/api/health");
			return response.headers.get("cache-control");
		}),
		"no-store",
	);
	assert.equal(
		await page.evaluate(async () => {
			const response = await fetch("/manifest.json");
			return response.headers.get("cache-control");
		}),
		"public, max-age=3600",
	);

	await page.evaluate(() => {
		document.documentElement.dataset.theme = "light";
	});
	await assertAccessible(page, "dashboard light");
	await page.evaluate(() => {
		document.documentElement.dataset.theme = "dark";
	});
	await assertAccessible(page, "dashboard dark");
	await page.evaluate(() => {
		document.documentElement.dataset.theme = "light";
	});

	// Switching to a placeholder fetches its fragment.
	await page.selectOption(".hn-select", "newest");
	await page.waitForSelector(
		'.stories[data-for-page="newest"][data-loaded="true"]',
	);
	assert.equal(
		await page.locator('.stories[data-for-page="newest"] .story').count(),
		1,
	);
	await page.click('.tab-labels label[for="gh-tab-weekly"]');
	await page.waitForSelector(
		'.tab-content.active ol.repos[data-for-lang="all"][data-loaded="true"]',
	);

	// Settings interactions.
	await page.goto(`${base}/settings`, { waitUntil: "networkidle" });
	await page.waitForSelector(".settings-grid");
	await page.click('.theme-btn[data-theme="light"]');
	assert.equal(
		await page.evaluate(() => document.documentElement.dataset.theme),
		"light",
	);
	assert.equal(
		await page
			.locator('.theme-btn[data-theme="light"]')
			.getAttribute("aria-pressed"),
		"true",
	);
	assert.equal(
		await page.evaluate(() =>
			document
				.querySelector('meta[name="theme-color"]')
				?.getAttribute("content"),
		),
		"#f5f5f5",
	);
	assert.match(await page.inputValue(".share-url"), /hn=newest/);

	const settingsEtag = await page.evaluate(async () => {
		const response = await fetch("/settings");
		return response.headers.get("etag");
	});
	assert.ok(settingsEtag, "settings response should carry an ETag");
	assert.equal(
		await page.evaluate(
			async (etag) =>
				(
					await fetch("/settings", {
						headers: { "If-None-Match": etag },
					})
				).status,
			settingsEtag,
		),
		304,
	);

	// The checkbox is visually hidden but must remain keyboard focusable.
	const firstSubreddit = page.locator(".sub-check").first();
	const wasChecked = await firstSubreddit.isChecked();
	await firstSubreddit.focus();
	await page.keyboard.press("Space");
	assert.equal(await firstSubreddit.isChecked(), !wasChecked);
	await page.keyboard.press("Space");

	const order = () =>
		page
			.locator(".panel-toggle")
			.evaluateAll((buttons) => buttons.map((button) => button.dataset.panel));
	await assertAccessible(page, "settings light");
	await page.evaluate(() => {
		document.documentElement.dataset.theme = "dark";
	});
	await assertAccessible(page, "settings dark");
	await page.evaluate(() => {
		document.documentElement.dataset.theme = "light";
	});

	const noJsContext = await browser.newContext({ javaScriptEnabled: false });
	const noJsPage = await noJsContext.newPage();
	await noJsPage.goto(`${base}/settings`, { waitUntil: "networkidle" });
	assert.equal(await noJsPage.locator(".settings-grid").isVisible(), false);
	assert.equal(await noJsPage.locator(".noscript-msg").isVisible(), true);
	await noJsContext.close();

	assert.deepEqual(await order(), ["hn", "gh", "reddit"]);
	await page.focus('.panel-toggle[data-panel="gh"]');
	await page.keyboard.press("ArrowUp");
	assert.deepEqual(await order(), ["gh", "hn", "reddit"]);
	await page.keyboard.press("ArrowDown");
	assert.deepEqual(await order(), ["hn", "gh", "reddit"]);

	// Native details toggle, with no JavaScript handler.
	await page.goto(`${base}/?smoke=details`, { waitUntil: "networkidle" });
	await page.waitForSelector(".tab-content.active .repo-desc");
	assert.equal(
		await page
			.locator(".tab-content.active .repo-desc")
			.first()
			.getAttribute("open"),
		null,
	);
	await page.locator(".tab-content.active .repo-desc summary").first().click();
	assert.notEqual(
		await page
			.locator(".tab-content.active .repo-desc")
			.first()
			.getAttribute("open"),
		null,
	);

	// Custom Reddit and GitHub lists.
	await page.evaluate(() => {
		const profile = JSON.parse(localStorage.getItem("tty1") || "{}");
		profile.subs = ["dotnet", "docker"];
		profile.langs = ["Rust", "Go"];
		localStorage.setItem("tty1", JSON.stringify(profile));
	});
	await page.goto(`${base}/?smoke=custom`, { waitUntil: "networkidle" });
	await page.waitForSelector(
		'.reddit-posts[data-for-sub="all"][data-custom="true"]',
	);
	assert.equal(
		await page
			.locator('.reddit-posts[data-for-sub="all"] .reddit-post')
			.count(),
		2,
	);
	assert.equal(
		await page.evaluate(async () => {
			const response = await fetch("/f/reddit/all?subs=ClaudeAI");
			return response.status;
		}),
		200,
	);

	// A private custom fragment is cached by the browser-local service worker
	// and remains available after going offline.
	await page.evaluate(async () => {
		await navigator.serviceWorker.ready;
	});
	await page.waitForTimeout(300);
	await page.context().setOffline(true);
	await page.reload({ waitUntil: "domcontentloaded" });
	await page.waitForSelector(
		'.reddit-posts[data-for-sub="all"][data-custom="true"]',
		{ timeout: 10000 },
	);
	assert.equal(
		await page
			.locator('.reddit-posts[data-for-sub="all"] .reddit-post')
			.count(),
		2,
	);
	await page.context().setOffline(false);

	await page.selectOption(".lang-select", "mine");
	await page.waitForSelector(
		'.tab-content.active .repos[data-for-lang="mine"][data-custom="true"]',
	);
	assert.ok(
		(await page.locator('.repos[data-for-lang="mine"] .repo').count()) > 0,
		"custom GitHub list should be non-empty",
	);

	// Mobile panel navigation.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto(`${base}/?smoke=mobile`, { waitUntil: "networkidle" });
	await page.waitForSelector(".dashboard");
	await page.locator(".settings-link").waitFor({ state: "visible" });
	await page.click(".dot-gh");
	await page.waitForTimeout(800);
	assert.ok(
		(await page.locator(".dashboard").evaluate((el) => el.scrollLeft)) > 0,
		"swipe dot should scroll the dashboard",
	);
	assert.equal(
		await page.locator(".dot-gh").getAttribute("aria-current"),
		"true",
	);

	const unexpected = errors.filter(
		(error) => !error.includes("ERR_INTERNET_DISCONNECTED"),
	);
	assert.deepEqual(unexpected, [], `browser errors:\n${unexpected.join("\n")}`);
	console.log("browser smoke: ok");
} finally {
	if (browser) await browser.close();
	if (server.exitCode === null) {
		server.kill("SIGTERM");
		await new Promise((resolve) => server.once("exit", resolve));
	}
	await rm(stateDir, { recursive: true, force: true });
}
