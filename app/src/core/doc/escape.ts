// Who may keep an Escape for itself. Escape closes the innermost thing first: a field, a menu, a dialog, an editor's
// pop-up keep it; the document area's own uses (leave focus mode, leave an editor, close a review) come after.

/** What may keep an Escape for itself: a field, a menu, a dialog, the editors' pop-ups. */
export const OWNS_ESCAPE =
  "input, select, textarea, form, [role=dialog], [role=listbox], [role=menu]";

/** Overlays that close on Escape wherever the keyboard is (a drop question, a menu, a dialog). */
const OPEN_OVERLAY =
  "[role=dialog], [role=alertdialog], [role=menu], [role=listbox], dialog[open], .slash, .cm-tooltip-autocomplete, .cm-search";

/** The key event's target sits in something that keeps Escape for itself. */
export function ownedByTarget(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(OWNS_ESCAPE) !== null;
}

/** An overlay is shown somewhere in `root` (it takes Escape first, even without the keyboard). */
export function overlayShown(root: ParentNode = document): boolean {
  for (const el of root.querySelectorAll(OPEN_OVERLAY)) {
    if (el instanceof HTMLDialogElement && !el.open) continue;
    if (el.closest("[hidden]") === null) return true;
  }
  return false;
}
