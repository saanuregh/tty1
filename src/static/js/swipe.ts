import { $, $$, load, update } from "./common.ts";

export function initSwipe(): void {
	const mobileQuery = window.matchMedia("(max-width: 899px)");
	let swipeObserver: IntersectionObserver | null = null;
	let dotController: AbortController | null = null;

	function initSwipe(): void {
		const dashboard = $(".dashboard");
		if (!dashboard) return;
		dotController?.abort();
		const controller = new AbortController();
		dotController = controller;

		const panels = $$(".panel:not(.panel-hidden)");
		const dots = $$(".swipe-dot:not(.dot-hidden)");

		const savedPanel = Number(load().panel) || 0;
		for (const [index, dot] of dots.entries()) {
			const active = index === savedPanel;
			dot.classList.toggle("active", active);
			dot.setAttribute("aria-current", String(active));
		}
		const savedPanelElement = panels[savedPanel];
		if (savedPanel > 0 && savedPanelElement) {
			savedPanelElement.scrollIntoView({
				inline: "start",
				block: "nearest",
				behavior: "instant" as ScrollBehavior,
			});
		}

		const observer = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					if (!entry.isIntersecting) continue;
					const index = Array.prototype.indexOf.call(panels, entry.target);
					for (const [dotIndex, dot] of dots.entries()) {
						const active = dotIndex === index;
						dot.classList.toggle("active", active);
						dot.setAttribute("aria-current", String(active));
					}
					update((profile) => {
						profile.panel = index;
					});
				}
			},
			{ root: dashboard, threshold: 0.5 },
		);
		swipeObserver = observer;
		for (const panel of panels) observer.observe(panel);

		for (const [index, dot] of dots.entries()) {
			dot.addEventListener(
				"click",
				() => {
					panels[index]?.scrollIntoView({
						behavior: "smooth",
						inline: "start",
					});
				},
				{ signal: controller.signal },
			);
		}
	}

	function teardownSwipe(): void {
		dotController?.abort();
		dotController = null;
		if (swipeObserver) {
			swipeObserver.disconnect();
			swipeObserver = null;
		}
	}

	if (mobileQuery.matches) initSwipe();
	mobileQuery.addEventListener("change", (event) => {
		if (event.matches) initSwipe();
		else teardownSwipe();
	});
}
