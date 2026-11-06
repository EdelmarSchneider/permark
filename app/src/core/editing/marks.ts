// Correction marks, the part both editors share (session `marks`; Source: `source/marks.ts`, Edit: `visual/marks.ts`):
// the classes a mark wears (look: `marks.css`) and the suggestion index a click landed on.

import "./marks.css";

/** The classes of a mark: all of them, the current one, an insertion (caret-width). */
export function markClass(current: boolean, insert = false): string {
  return [
    "correction-mark",
    current ? "correction-mark--current" : "",
    insert ? "correction-mark--insert" : "",
  ]
    .filter((c) => c !== "")
    .join(" ");
}

/** The index a mark carries (`data-correction-mark`) for the element clicked; `null`: the click was not on a mark. */
export function clickedMark(target: EventTarget | null): number | null {
  const el = target instanceof Element ? target.closest("[data-correction-mark]") : null;
  const value = el?.getAttribute("data-correction-mark");
  const index = value === null || value === undefined ? NaN : Number(value);
  return Number.isInteger(index) ? index : null;
}
