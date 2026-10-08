import { tick } from "svelte";

/**
 * Scrolls the element to the top of its scroll area once the DOM has updated and laid out. Uses
 * "start" because the panels are taller than the viewport, where "nearest" doesn't move.
 * Takes a getter because the element is often rendered by the same state change that triggers
 * the scroll, so it doesn't exist yet at call time. Does nothing without an element or where
 * `scrollIntoView` is missing (jsdom).
 */
export async function scrollIntoViewSoon(getEl: () => Element | null | undefined): Promise<void> {
  await tick();
  if (typeof requestAnimationFrame !== "undefined") {
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }
  getEl()?.scrollIntoView?.({ block: "start", behavior: "smooth" });
}
