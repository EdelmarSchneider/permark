// Closing a tab. Anywhere (its chip's close button, its menu, Mod+W) asks the backend; a tab with unsaved edits comes
// forward and its view is asked (`tab-close-requested`): `closeHere` settles the edits first (the unsaved prompt),
// then confirms. The last tab leaves the window on the Welcome.

import { api, type Api } from "../../api";
import { confirmLeave } from "../editing/guard";
import { windowStore as store, type WindowStore } from "../window.svelte";

/** Closes the active tab, the one on screen (Mod+W); a window without tabs (the Welcome) closes nothing. */
export function closeOwnTab(deps: { store: WindowStore } = { store }): void {
  void deps.store.closeWorkspace();
}

/** This view's tab is to close: its unsaved edits are settled first (Cancel -> it stays). */
export async function closeHere(deps: { api: Api } = { api }): Promise<void> {
  if (!(await confirmLeave({ kind: "workspace" }))) return;
  await deps.api.tabCloseConfirmed();
}
