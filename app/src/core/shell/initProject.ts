// "Initialize PerMark": asks first (what is created), then the backend makes `.permark/` and the open project reloads.

import { windowStore } from "../window.svelte";
import { ui } from "./ui.svelte";

/** Asks whether to initialize PerMark in the project this window shows (nothing when it shows none). */
export function askInitHere(): void {
  const view = windowStore.view;
  if (view.state !== "showing" || view.workspace.kind !== "project") return;
  ui.askInit(view.workspace.root, view.workspace.name);
}

/** The person said yes. */
export async function confirmInit(path: string): Promise<void> {
  ui.closeInit();
  await windowStore.initProject(path);
}
