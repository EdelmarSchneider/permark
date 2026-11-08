// View | Edit | Source for the document the window shows: one place for the header buttons and the commands.

import { documents } from "../doc/document.svelte";
import type { DocumentView } from "../../api";
import { ui } from "../shell/ui.svelte";
import { editing, type EditKind, type EditMode } from "./session.svelte";

/** Why the open document cannot be edited (`null`: it can; no document on screen yet: `noViewer`). */
export function editBlockedReason(): string | null {
  // not the previous file's view while the next one loads: an editor chosen from it would be the wrong kind
  const view = documents.current;
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
  const view = documents.current;
  if (shown === null || view === null || editBlockedReason() !== null) return;
  if (editing.mode === "view" || editing.path !== shown) {
    void editing.enter(mode, shown, editKindOf(view), documents.savePolicy, at);
  } else void editing.switchTo(mode, at);
}

/**
 * Opens the document the window shows in the mode chosen in this window (a file switch keeps the choice; View
 * needs nothing). An editor opens only when the document allows it, and never twice on the same document.
 */
export function openRemembered(): void {
  const mode = editing.chosenMode;
  const shown = documents.path;
  const view = documents.current;
  if (mode === "view" || shown === null || view === null) return;
  // the editor is already on this document
  if (editing.path === shown) return;
  if (editBlockedReason() !== null) return;
  void editing.enter(mode, shown, editKindOf(view), documents.savePolicy);
}

/** Find in page has something to search: the page, Edit's text, or Source (its own panel). */
export function canFind(): boolean {
  if (documents.path === null) return false;
  return !editingNow() || editing.finder !== null || editing.mode === "source";
}

/** Find in page in the mode on screen: the find bar over the page | Edit's text; Source opens its own panel. */
export function openFind(): void {
  if (!canFind()) return;
  if (editingNow() && editing.finder === null) editing.search();
  else ui.openFind();
}
