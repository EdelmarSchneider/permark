// The import flow of this window: the real backend, the window's notices, the tree and the editor it points at.

import { api } from "../../api";
import { documents } from "../doc/document.svelte";
import { editing } from "../editing/session.svelte";
import { windowStore } from "../window.svelte";
import { markTreeDrop, treeFolderAt, undoInTree } from "./current";
import { spotAt } from "./dropTarget";
import { ImportFlow } from "./importFlow.svelte";

export const importFlow = new ImportFlow(api, {
  fail: (error) => windowStore.notifyFileOpFailed(error),
  note: (text) => windowStore.notifyNote(text),
  action: (text, ...actions) => windowStore.notifyAction(text, ...actions),
  project: () => {
    const view = windowStore.view;
    return view.state === "showing" ? view.workspace.name : "";
  },
  spot: (x, y) => spotAt(x, y, { treeFolder: treeFolderAt, document: () => documents.path }),
  mark: markTreeDrop,
  pictures: (files, x, y) => editing.dropPictures(files, { x, y }),
  undo: undoInTree,
});
