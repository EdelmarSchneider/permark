// The projects store of this view, wired to the window's notices and the settings page. The window store is loaded
// when a notice is due (not with the Explorer: it brings the whole window state along).

import { tick } from "svelte";
import { api } from "../../api";
import { ui } from "../shell/ui.svelte";
import { ProjectsStore } from "./store.svelte";

async function windowStore(): Promise<typeof import("../window.svelte").windowStore> {
  return (await import("../window.svelte")).windowStore;
}

/** Focus that fell to the page (its button went away) goes to the strip's first button, else the file list; while the
 *  settings page shows: to its section list. */
export async function refocusExplorer(): Promise<void> {
  await tick();
  const at = document.activeElement;
  if (at !== null && at !== document.body && at.isConnected) return;
  const target =
    ui.settingsPage.status === "open"
      ? document.querySelector<HTMLElement>('[role="dialog"] nav [aria-current="page"]')
      : (document.querySelector<HTMLElement>("[data-projects-strip] button:not([disabled])") ??
        document.querySelector<HTMLElement>('[role="tree"]'));
  target?.focus();
}

export const projects = new ProjectsStore(api, {
  notice: (notice) => void windowStore().then((w) => w.notifyTimed(notice)),
  fail: (text) => void windowStore().then((w) => w.notifyNote(text)),
  openSettings: () => ui.openSettingsPage("projects"),
  closeSettings: () => ui.closeSettingsPage(),
  settingsShown: () => ui.settingsPage.status === "open",
  refocus: () => void refocusExplorer(),
});
