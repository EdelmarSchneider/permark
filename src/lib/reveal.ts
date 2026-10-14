// Showing the open document in the Explorer (docs/design/v0.2.md section 16): which folders to open, which
// section of the Explorer holds the file, and whether a newly opened document is revealed at all.

import { WT_PREFIX, ancestors } from "./paths";

export type ExplorerSection = "vault" | "worktrees";

export type RevealPlan = {
  /** The row to select and scroll into view. */
  target: string;
  /** Folders to open so the row is there, outermost first; folders that are already open are left out. */
  expand: string[];
  /** The Explorer section the row lives in (a collapsed section hides it). */
  section: ExplorerSection;
};

/** What it takes to make `path` visible in the Explorer, given the folders that are open now. */
export function planReveal(path: string, expanded: ReadonlySet<string>): RevealPlan {
  return {
    target: path,
    expand: ancestors(path).filter((dir) => !expanded.has(dir)),
    // a worktree's files are listed under its root, in their own section
    section: path.startsWith(WT_PREFIX) ? "worktrees" : "vault",
  };
}

export type OpenedDoc = {
  /** The "Always select the opened file" setting. */
  autoReveal: boolean;
  /** Simple mode has no Explorer. */
  simple: boolean;
  /** The document was already the open one, and the request only moved within it. */
  movedWithin: boolean;
};

/** Whether a document that was just opened is revealed in the Explorer on its own. */
export function shouldAutoReveal(opened: OpenedDoc): boolean {
  return opened.autoReveal && !opened.simple && !opened.movedWithin;
}

/**
 * Where to scroll a list so a row is in view with the least movement: null when it is fully visible
 * already (nothing moves), the row's top edge when it is above the view, its bottom edge when it is below.
 * All numbers are in the list's own coordinates; `margin` keeps the row off the edge it scrolls to.
 */
export function nearestScrollTop(
  row: { top: number; bottom: number },
  view: { scrollTop: number; height: number },
  margin = 0,
): number | null {
  if (row.top < view.scrollTop) return Math.max(0, row.top - margin);
  if (row.bottom > view.scrollTop + view.height) return row.bottom + margin - view.height;
  return null;
}
