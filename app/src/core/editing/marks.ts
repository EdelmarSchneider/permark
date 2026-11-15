// Marks in the editors, the part both share (session `marks`; Source: `source/marks.ts`, Edit: `visual/marks.ts`):
// the classes and attributes a mark wears (look: `marks.css`), the layer + index a click landed on, and the click |
// context menu routing to the layer that owns the mark. Layers are opaque ids; each one's marks never touch another's.

import "./marks.css";

/** The layer a plain `marks` call draws on (the one the whole-text review uses). */
export const DEFAULT_LAYER = "correction";

/** How a layer's marks look and answer, besides a click (`pick`). */
export type MarkOptions = {
  /** Base class of the layer's marks (`<base>`, `<base>--current`, `<base>--insert`); none: the review's look. */
  className?: string;
  /** Right-click on one of the layer's marks (its index, client point); the webview's own menu then stays shut. */
  menu?: (index: number, x: number, y: number) => void;
};

/** What a layer answers to on screen. */
export type MarkHandlers = {
  pick: (index: number) => void;
  menu: ((index: number, x: number, y: number) => void) | null;
};

/** A mark found under an element: its layer and its index in that layer's last `marks`. */
export type MarkHit = { layer: string; index: number };

/** The classes of a mark: all of them, the current one, an insertion (caret-width). */
export function markClass(current: boolean, insert = false, base = "correction-mark"): string {
  return [base, current ? `${base}--current` : "", insert ? `${base}--insert` : ""]
    .filter((c) => c !== "")
    .join(" ");
}

/** The attributes of a mark. The default layer also keeps `data-correction-mark`, the attribute its callers read. */
export function markAttrs(layer: string, index: number): Record<string, string> {
  return {
    "data-mark-layer": layer,
    "data-mark-index": String(index),
    ...(layer === DEFAULT_LAYER ? { "data-correction-mark": String(index) } : {}),
  };
}

/** The marks holding `target`, nearest first (marks of several layers may nest). */
function marksUnder(target: EventTarget | null): MarkHit[] {
  const hits: MarkHit[] = [];
  let el = target instanceof Element ? target.closest("[data-mark-layer]") : null;
  while (el !== null) {
    const layer = el.getAttribute("data-mark-layer");
    const index = Number(el.getAttribute("data-mark-index") ?? NaN);
    if (layer !== null && Number.isInteger(index)) hits.push({ layer, index });
    el = el.parentElement?.closest("[data-mark-layer]") ?? null;
  }
  return hits;
}

/** The mark under the element clicked (`layer`: the nearest of that layer); `null`: none. */
export function clickedMark(target: EventTarget | null, layer?: string): MarkHit | null {
  return marksUnder(target).find((hit) => layer === undefined || hit.layer === layer) ?? null;
}

/** A click picks the clicked mark in every layer holding one there; the caret goes where the person clicked. */
export function pickClicked(
  layers: ReadonlyMap<string, MarkHandlers>,
  target: EventTarget | null,
): void {
  const picked = new Set<string>();
  for (const hit of marksUnder(target)) {
    if (picked.has(hit.layer)) continue;
    picked.add(hit.layer);
    layers.get(hit.layer)?.pick(hit.index);
  }
}

/** A right-click on a mark of a layer with a menu opens it (nearest such mark); `true`: taken. */
export function menuClicked(layers: ReadonlyMap<string, MarkHandlers>, event: MouseEvent): boolean {
  for (const hit of marksUnder(event.target)) {
    const menu = layers.get(hit.layer)?.menu;
    if (!menu) continue;
    event.preventDefault();
    menu(hit.index, event.clientX, event.clientY);
    return true;
  }
  return false;
}
