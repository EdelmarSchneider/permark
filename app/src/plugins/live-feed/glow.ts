// Changed passages glow while an agent writes; a removed passage leaves a thin marker where it was. Classes and
// marker elements only; the animation is CSS (`glow.css`, opacity). Fresh documents never glow.

import { documents, type BlockUpdate } from "../../core/doc/document.svelte";
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

/** Starts glowing for the page of this window; returns what stops it (and clears what is on screen). */
export function startGlow(): () => void {
  const timers = new Set<ReturnType<typeof setTimeout>>();
  const off = documents.onUpdated((update) =>
    mark(update, documents.host.container, (id) => documents.host.element(id), timers),
  );
  return () => {
    off();
    for (const timer of timers) clearTimeout(timer);
    timers.clear();
    const container = documents.host.container;
    for (const el of container?.querySelectorAll(".pm-glow, .pm-removed-mark") ?? []) {
      if (el.classList.contains("pm-removed-mark")) el.remove();
      else el.classList.remove("pm-glow");
    }
  };
}
