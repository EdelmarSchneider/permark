// The selection in the reading view, as features that hand a passage to an agent take it: its text and the lines of
// the blocks it starts and ends in.

import { documents } from "./document.svelte";

/** The page selection: its text, and the first lines of the blocks where it starts and ends (`null`: not known). */
export type PageSelection = { text: string; lines: [number, number] | null } | null;

/** The selection in the page (reading view): its text and lines; `null` when collapsed or outside the page. */
export function pageSelection(): PageSelection {
  const selection = document.getSelection();
  if (selection === null || selection.isCollapsed) return null;
  const inPage = (node: Node | null): boolean => {
    const element = node instanceof Element ? node : (node?.parentElement ?? null);
    return element?.closest("[data-b]") != null;
  };
  if (!inPage(selection.anchorNode) || !inPage(selection.focusNode)) return null;
  const a = documents.referenceAt(selection.anchorNode)?.line ?? null;
  const b = documents.referenceAt(selection.focusNode)?.line ?? null;
  const lines: [number, number] | null =
    a !== null && b !== null ? [Math.min(a, b), Math.max(a, b)] : null;
  return { text: selection.toString(), lines };
}
