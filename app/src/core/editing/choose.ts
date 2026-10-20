// View | Edit | Source for the document the window shows: one place for the header buttons and the commands.

import { documents } from "../doc/document.svelte";
import type { DocumentView } from "../../api";
import { editing, type EditKind, type EditMode } from "./session.svelte";

/** Why the open document cannot be edited (`null`: it can; no document: `noViewer`). */
export function editBlockedReason(): string | null {
  const view = documents.view;
  if (view === null) return "noViewer";
  return view.editable.editable === "readOnly" ? view.editable.reason : null;
}

/** What Edit is for a document: the visual editor, the sheet, or nothing (Source only). */
export function editKindOf(view: DocumentView): EditKind {
  if (view.format === "markdown") return "markdown";
  return view.body.kind === "sheet" ? "sheet" : "text";
}

/** An editor (Edit | Source) is open on the document the window shows. */
export function editingNow(): boolean {
  return editing.mode !== "view" && editing.path === documents.path;
}

/** Switches the window to a mode (`at`: a line to start at); an editor opens only when the document allows it. */
export function chooseMode(mode: EditMode, at?: number): void {
  const shown = documents.path;
  if (mode === "view") {
    void editing.switchTo("view");
    return;
  }
  const view = documents.view;
  if (shown === null || view === null || editBlockedReason() !== null) return;
  if (editing.mode === "view" || editing.path !== shown) {
    void editing.enter(mode, shown, editKindOf(view), documents.savePolicy, at);
  } else void editing.switchTo(mode, at);
}
