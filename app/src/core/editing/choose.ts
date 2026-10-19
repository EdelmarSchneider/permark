// View | Edit | Source for the document the window shows: one place for the header buttons and the commands.

import { documents } from "../doc/document.svelte";
import { editing, type EditMode } from "./session.svelte";

/** Why the open document cannot be edited (`null`: it can; no document: `noViewer`). */
export function editBlockedReason(): string | null {
  const view = documents.view;
  if (view === null) return "noViewer";
  return view.editable.editable === "readOnly" ? view.editable.reason : null;
}

/** An editor (Edit | Source) is open on the document the window shows. */
export function editingNow(): boolean {
  return editing.mode !== "view" && editing.path === documents.path;
}

/** Switches the window to a mode; an editor opens only when the document allows it. */
export function chooseMode(mode: EditMode): void {
  const shown = documents.path;
  if (mode === "view") {
    void editing.switchTo("view");
    return;
  }
  const view = documents.view;
  if (shown === null || view === null || editBlockedReason() !== null) return;
  if (editing.mode === "view" || editing.path !== shown) {
    void editing.enter(mode, shown, view.format === "markdown", documents.savePolicy);
  } else void editing.switchTo(mode);
}
