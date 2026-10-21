// The projects store of this view, wired to the window's notices and the settings page. The window store is loaded
// when a notice is due (not with the Explorer: it brings the whole window state along).

import { api } from "../../api";
import { ui } from "../shell/ui.svelte";
import { ProjectsStore, type NoticeAction } from "./store.svelte";

async function windowStore(): Promise<typeof import("../window.svelte").windowStore> {
  return (await import("../window.svelte")).windowStore;
}

export const projects = new ProjectsStore(api, {
  note: (text) => void windowStore().then((w) => w.notifyNote(text)),
  action: (text, ...actions: NoticeAction[]) =>
    void windowStore().then((w) => w.notifyAction(text, ...actions)),
  fail: (text) => void windowStore().then((w) => w.notifyNote(text)),
  openSettings: () => ui.openSettingsPage("projects"),
  closeSettings: () => ui.closeSettingsPage(),
});
