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

/** Starts glowing for the page of this window; returns what stops it (and clears what is on screen). */
export function startGlow(): () => void {
  const timers = new Set<ReturnType<typeof setTimeout>>();
  const off = documents.onUpdated((update) =>
    mark(update, documents.host.container, (id) => documents.host.element(id), timers),
  );
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
