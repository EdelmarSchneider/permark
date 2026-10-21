// The import flow of this view: the real backend, the window's notices, the tree it lands in.

import { api } from "../../api";
import { onScreen } from "../inFront.svelte";
import { windowStore } from "../window.svelte";
import { treeDrop, undoEntryInTree } from "./current";
import { ImportFlow } from "./importFlow.svelte";

export const importFlow = new ImportFlow(api, {
  fail: (error) => windowStore.notifyFileOpFailed(error),
  note: (text) => windowStore.notifyNote(text),
  action: (text, ...actions) => windowStore.notifyAction(text, ...actions),
  project: () => {
    const view = windowStore.view;
    return view.state === "showing" ? view.workspace.name : "";
  },
  view: () => windowStore.label,
  focus: () => void api.focusView(),
  shown: onScreen,
  end: treeDrop.end,
  placed: treeDrop.placed,
  undo: undoEntryInTree,
});
