import { $, $$, getTheme, isTyping, setTheme } from "./common.ts";

let activePanel = 0;
let focusedIdx = -1;

function getVisibleItems(panelIndex: number): HTMLLIElement[] {
	const panel = $$(".panel")[panelIndex];
	if (!panel) return [];
	for (const list of panel.querySelectorAll<HTMLOListElement>("ol")) {
		if (list.style.display !== "none" && list.offsetParent !== null)
			return Array.from(list.querySelectorAll<HTMLLIElement>("li"));
	}
	return [];
}

function clearFocus(): void {
	$(".focused")?.classList.remove("focused");
}

function orderedVisiblePanels(): number[] {
	const panels = $$(".panel");
	const visible: number[] = [];
	for (const [index, panel] of panels.entries()) {
		if (!panel.classList.contains("panel-hidden")) visible.push(index);
	}
	visible.sort((a, b) => {
		const first = Number.parseInt(panels[a]?.style.order ?? "", 10) || 0;
		const second = Number.parseInt(panels[b]?.style.order ?? "", 10) || 0;
		return first - second;
	});
	return visible;
}

function setFocus(index: number): void {
	clearFocus();
	const items = getVisibleItems(activePanel);
	if (index < 0 || index >= items.length) return;
	focusedIdx = index;
	const item = items[index];
	if (!item) return;
	item.classList.add("focused");
	item.scrollIntoView({ block: "nearest" });
}

function getFocusedItem(): HTMLLIElement | null {
	const items = getVisibleItems(activePanel);
	return focusedIdx >= 0 && focusedIdx < items.length
		? (items[focusedIdx] ?? null)
		: null;
}

function navigate(
	anchor: HTMLAnchorElement | null | undefined,
	newTab: boolean,
) {
	if (!anchor) return;
	if (newTab)
		anchor.dispatchEvent(
			new MouseEvent("click", { ctrlKey: true, bubbles: true }),
		);
	else window.location.href = anchor.href;
}

function openFocusedLink(newTab: boolean): void {
	navigate(getFocusedItem()?.querySelector("a"), newTab);
}

function openFocusedComments(newTab: boolean): void {
	const item = getFocusedItem();
	if (!item) return;
	const links = Array.from(item.querySelectorAll<HTMLAnchorElement>("a"));
	const link = links.length > 1 ? links[links.length - 1] : links[0];
	navigate(link, newTab);
}

function onKeyDown(event: KeyboardEvent): void {
	if (event.key === "Escape" && document.activeElement instanceof HTMLElement) {
		document.activeElement.blur();
		return;
	}
	if (event.target instanceof HTMLSelectElement) {
		if (event.key === "j" || event.key === "k") {
			event.preventDefault();
			const index = event.target.selectedIndex + (event.key === "j" ? 1 : -1);
			if (index >= 0 && index < event.target.options.length) {
				event.target.selectedIndex = index;
				event.target.dispatchEvent(new Event("change", { bubbles: true }));
			}
			return;
		}
		if (event.key === "h" || event.key === "l") event.target.blur();
		else return;
	}
	if (isTyping(event)) return;

	if (event.key === "h" || event.key === "l") {
		event.preventDefault();
		const panels = $$(".panel");
		const visible = orderedVisiblePanels();
		if (visible.length === 0) return;
		const current = visible.indexOf(activePanel);
		const delta = event.key === "h" ? -1 : 1;
		const next =
			current === -1 ? 0 : (current + delta + visible.length) % visible.length;
		const nextPanel = visible[next];
		if (nextPanel === undefined) return;
		activePanel = nextPanel;
		resetFocus();
		for (const [index, panel] of panels.entries())
			panel.classList.toggle("active-panel", index === activePanel);
	} else if (event.key === "f") {
		event.preventDefault();
		const panel = $$(".panel")[activePanel];
		if (!panel) return;
		const controls = panel.querySelectorAll<HTMLElement>(
			"select, .tab-labels label",
		);
		if (controls.length === 0) return;
		const current = Array.prototype.indexOf.call(
			controls,
			document.activeElement,
		);
		controls.item((current + 1) % controls.length)?.focus();
	} else if (event.key === "j") {
		event.preventDefault();
		const items = getVisibleItems(activePanel);
		if (items.length > 0) setFocus(Math.min(focusedIdx + 1, items.length - 1));
	} else if (event.key === "k") {
		event.preventDefault();
		if (focusedIdx > 0) setFocus(focusedIdx - 1);
	} else if (event.key === "Enter" && focusedIdx >= 0) {
		event.preventDefault();
		openFocusedLink(event.shiftKey);
	} else if (event.key === "c" && focusedIdx >= 0) {
		event.preventDefault();
		openFocusedComments(event.shiftKey);
	} else if (event.key === "r") {
		event.preventDefault();
		window.location.reload();
	} else if (event.key === "t") {
		setTheme(getTheme() === "dark" ? "light" : "dark");
	} else if (event.key === ",") {
		window.location.href = "/settings";
	}
}

export function resetFocus(): void {
	clearFocus();
	focusedIdx = -1;
}

export function initKeyboard(): void {
	const visible = orderedVisiblePanels();
	if (visible.length > 0) activePanel = visible[0] ?? activePanel;
	for (const [index, panel] of $$(".panel").entries())
		panel.classList.toggle("active-panel", index === activePanel);
	document.addEventListener("keydown", onKeyDown);
}
