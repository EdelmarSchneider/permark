// Closing a workspace. Anywhere (a chip's close button, its menu, Mod+W) asks the backend, which asks every window
// that shows it; each window then runs `closeHere`: unsaved edits are settled first, then the window closes when
// other workspaces remain open (the focused one moves to the next first), or goes back to the Welcome when none does.

import { api, type Api } from "../../api";
import { confirmLeave } from "../editing/guard";
import { windowStore as store, type WindowStore } from "../window.svelte";
import { shownId } from "./switcherModel";
import { switcher, type Switcher } from "./workspaceList.svelte";

/** Closes the workspace this window shows (every window that shows it is asked). */
export function closeOwnWorkspace(): void {
  const id = shownId(store.view);
  if (id !== null) switcher.close(id);
}

type Deps = { api: Api; store: WindowStore; switcher: Switcher; focused: () => boolean };

/** This window was asked to close its workspace. */
export async function closeHere(
  deps: Deps = { api, store, switcher, focused: () => document.hasFocus() },
): Promise<void> {
  const id = shownId(deps.store.view);
  if (!(await confirmLeave({ kind: "workspace" }))) return;
  const next = id === null ? null : deps.switcher.afterClose(id);
  if (next === null) {
    await deps.store.closeWorkspace();
    return;
  }
  // the person was here: they land on the next workspace, not on whatever window the system picks
  if (deps.focused()) deps.switcher.focus(next.id);
  await deps.api.windowClose();
}
