// "Initialize PerMark": asks first (what is created), then the backend makes `.permark/` and the open project reloads.

import { folderName } from "../projects/model";
import { windowStore, type WindowStore } from "../window.svelte";
import { ui } from "./ui.svelte";

/** Asks whether to initialize PerMark in the project this window shows (nothing when it shows none). */
export function askInitHere(): void {
  const view = windowStore.view;
  if (view.state !== "showing" || view.workspace.kind !== "project") return;
  ui.askInit(view.workspace.root, view.workspace.name);
}

/** New project: a folder is picked and set up, then opens; a folder with files in it is asked about first. */
export async function newProject(store: WindowStore = windowStore): Promise<void> {
  const asked = await store.newProject();
  if (asked !== null) ui.askInit(asked.ask, folderName(asked.ask), true);
}

/** The person said yes; `opens`: the folder opens once set up (New project). */
export async function confirmInit(path: string, opens = false): Promise<void> {
  ui.closeInit();
  if (opens) await windowStore.createProject(path);
  else await windowStore.initProject(path);
}
