// Changed passages glow while an agent writes; a removed passage leaves a thin marker where it was. Classes and
// marker elements only; the animation is CSS (`glow.css`, opacity). Fresh documents never glow.

import { documents, type BlockUpdate, type TextUpdate } from "../../core/doc/document.svelte";
import { t } from "../../core/i18n";
import "./glow.css";

/** How long a passage glows (ms): a little over the animation. */
export const GLOW_MS = 3000;
/** How long a marker for a removed passage stays (ms). */
export const MARKER_MS = 8000;

/** Marks the blocks of one update. `container`: the page; `element`: the block with an id. */
export function mark(
  update: BlockUpdate,
  container: HTMLElement | null,
  element: (id: string) => HTMLElement | undefined,
  timers: Set<ReturnType<typeof setTimeout>>,
): void {
  if (container === null) return;
  for (const old of container.querySelectorAll(".pm-removed-mark")) old.remove();
  for (const id of update.added) {
    const el = element(id);
    if (!el) continue;
    el.classList.add("pm-glow");
    const timer = setTimeout(() => {
      el.classList.remove("pm-glow");
      timers.delete(timer);
    }, GLOW_MS);
    timers.add(timer);
  }
  for (const gone of update.removed) {
    const marker = container.ownerDocument.createElement("div");
    marker.className = "pm-removed-mark";
    marker.setAttribute("role", "note");
    marker.title = t("glow.removed");
    marker.addEventListener("click", () => marker.remove());
    const before = gone.after === null ? undefined : element(gone.after);
    if (before) before.after(marker);
    else container.prepend(marker);
    const timer = setTimeout(() => {
      marker.remove();
      timers.delete(timer);
    }, MARKER_MS);
    timers.add(timer);
  }
}

/**
 * The lines of a text document that changed glow (the text viewer draws what `documents.lineGlow` says), and a thin
 * mark stays where lines went, longer. A newer update takes the place of the one on screen.
 */
export function markLines(update: TextUpdate, timers: Set<ReturnType<typeof setTimeout>>): void {
  for (const timer of timers) clearTimeout(timer);
  timers.clear();
  const { path, added, removedAfter } = update;
  documents.lineGlow = { path, added, removedAfter };
  const fade = setTimeout(() => {
    timers.delete(fade);
    if (documents.lineGlow?.path === path) documents.lineGlow = { path, added: [], removedAfter };
  }, GLOW_MS);
  const clear = setTimeout(() => {
    timers.delete(clear);
    if (documents.lineGlow?.path === path) documents.lineGlow = null;
  }, MARKER_MS);
  timers.add(fade);
  timers.add(clear);
}

/** The passages that glow now: blocks, the marks of removed ones, and the rectangles + marks of a text view. */
const GLOWING = ".pm-glow, .pm-removed-mark, .glows .glow, .glows .gone";
/** Where a step puts the passage it moves to: this share of the pane's height below its top edge. */
const LANDING = 0.2;
/** A passage within this many pixels of the landing line counts as the one already there. */
const HERE = 8;

/**
 * The passage a step moves to. `tops`: where each glowing passage starts, in page order, relative to the pane's top
 * edge. Next = the first below the landing line, previous = the last above it; past the last (or the first) the
 * step wraps. Null when nothing glows.
 */
export function pickGlow(
  tops: readonly number[],
  direction: 1 | -1,
  landing: number,
): number | null {
  if (tops.length === 0) return null;
  if (direction === 1) {
    const next = tops.findIndex((top) => top > landing + HERE);
    return next === -1 ? 0 : next;
  }
  const before = tops.findLastIndex((top) => top < landing - HERE);
  return before === -1 ? tops.length - 1 : before;
}

/** The first element of each run of neighbors: a stretch of glowing blocks is one change. */
function starts(found: readonly HTMLElement[]): HTMLElement[] {
  const all = new Set(found);
  return found.filter((el) => {
    const before = el.previousElementSibling;
    return !(before instanceof HTMLElement && all.has(before));
  });
}

/**
 * F7 | Shift+F7 while passages glow: scrolls the next | previous one to the landing line. False when none glows (a
 * glow lasts seconds, so the review of what was read is the lasting way to walk the changes).
 */
export function stepGlow(direction: 1 | -1): boolean {
  const pane = documents.host.scroller ?? document.querySelector<HTMLElement>("[data-scroller]");
  if (pane === null) return false;
  const found = starts([...pane.querySelectorAll<HTMLElement>(GLOWING)]);
  const edge = pane.getBoundingClientRect().top;
  const landing = LANDING * pane.clientHeight;
  const tops = found.map((el) => el.getBoundingClientRect().top - edge);
  const at = pickGlow(tops, direction, landing);
  if (at === null) return false;
  pane.scrollTo({
    top: Math.max(0, pane.scrollTop + (tops[at] ?? 0) - landing),
    behavior: "instant",
  });
  return true;
}

/**
 * Starts glowing for the page of this window; returns what stops it (and clears what is on screen). `glowed`: the
 * blocks of the shown document that started to glow (the outline marks their sections).
 */
export function startGlow(
  glowed: (path: string, ids: readonly string[]) => void = () => {},
): () => void {
  const timers = new Set<ReturnType<typeof setTimeout>>();
  const off = documents.onUpdated((update) => {
    mark(update, documents.host.container, (id) => documents.host.element(id), timers);
    const path = documents.path;
    if (path !== null && documents.host.container !== null) glowed(path, update.added);
  });
  const offText = documents.onTextUpdated((update) => markLines(update, timers));
  return () => {
    off();
    offText();
    documents.lineGlow = null;
    for (const timer of timers) clearTimeout(timer);
    timers.clear();
    const container = documents.host.container;
    for (const el of container?.querySelectorAll(".pm-glow, .pm-removed-mark") ?? []) {
      if (el.classList.contains("pm-removed-mark")) el.remove();
      else el.classList.remove("pm-glow");
    }
  };
}
