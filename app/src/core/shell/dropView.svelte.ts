// The drop flow of this view: the real backend, what the view shows, the tree, the import question, the editor.

import { api } from "../../api";
import { editing } from "../editing/session.svelte";
import { treeDrop, treeFolderAt } from "../explorer/current";
import { spotAt } from "../explorer/dropTarget";
import { folderLabel } from "../explorer/importModel";
import { importFlow } from "../explorer/importWindow.svelte";
import { onScreen } from "../inFront.svelte";
import { windowStore } from "../window.svelte";
import { DropFlow } from "./dropFlow.svelte";

export const dropFlow = new DropFlow(api, {
  workspace: () => {
    const view = windowStore.view;
    switch (view.state) {
      case "showing":
        return view.workspace.kind;
      case "opening":
        return view.showing?.kind ?? "welcome";
      case "welcome":
      case "closing":
        return "welcome";
      default:
        return view satisfies never;
    }
  },
  spot: (x, y) =>
    spotAt(x, y, { treeFolder: treeFolderAt, editorTakesPictures: () => editing.takesPictures() }),
  folderName: (folder) => {
    const view = windowStore.view;
    return folderLabel(folder, view.state === "showing" ? view.workspace.name : "");
  },
  tree: treeDrop,
  imports: importFlow,
  pictures: (files, at) => editing.dropPictures(files, at),
  fail: (error) => windowStore.notifyFileOpFailed(error),
  note: (text) => windowStore.notifyNote(text),
  shown: onScreen,
});
