// The selection in the reading view, as features that hand a passage to an agent take it: its text and the lines of
// the blocks it starts and ends in.

import { documents } from "./document.svelte";

/** The page selection: its text, and the first lines of the blocks where it starts and ends (`null`: not known). */
export type PageSelection = { text: string; lines: [number, number] | null } | null;

/** The selection in the page (reading view): its text and lines; `null` when collapsed or outside the page. */
export function pageSelection(): PageSelection {
  const selection = document.getSelection();
  if (selection === null || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (blockOf(range.startContainer) === null) return null;
  let end: Node | null = range.endContainer;
  if (blockOf(end) === null) {
    // a triple-click on the page's last block ends the selection at the very start of what follows the page (a
    // panel): no character of it is selected, so the selection still ends in the last page block it reaches
    if (range.endOffset !== 0) return null;
    end =
      [...document.querySelectorAll("[data-b]")].filter((b) => range.intersectsNode(b)).at(-1) ??
      null;
    if (end === null) return null;
  }
  const a = documents.referenceAt(range.startContainer)?.line ?? null;
  const b = documents.referenceAt(end)?.line ?? null;
  const lines: [number, number] | null =
    a !== null && b !== null ? [Math.min(a, b), Math.max(a, b)] : null;
  return { text: selection.toString(), lines };
}

/** The page selection kept while a box has the keyboard: focus in its field ends the page's own (`null`: none kept). */
let held: PageSelection = null;

/** Keeps `selection` for the commands a box lists and runs (`selectionForCommand`); `null` lets it go. */
export function holdSelection(selection: PageSelection): void {
  held = selection;
}

/** The selection a command acts on: the page's own, else the one kept while the palette lists | runs commands. */
export function selectionForCommand(): PageSelection {
  return pageSelection() ?? held;
}

function blockOf(node: Node | null): Element | null {
  const element = node instanceof Element ? node : (node?.parentElement ?? null);
  return element?.closest("[data-b]") ?? null;
}
