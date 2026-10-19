// Where a drop from outside would land: the part of the window under the pointer says so.

import { parentOf } from "./list";

/** The folder a drop lands in (`""` = the project folder) and whether the pointer is over an editor. */
export type Spot = { folder: string; editor: boolean };

export type SpotEnv = {
  /** The element under a point (the document's by default). */
  at?: (x: number, y: number) => Element | null;
  /** The tree's answer for a point over its list (`null`: over its header or elsewhere). */
  treeFolder: (x: number, y: number) => string | null;
  /** The open document, if one is shown. */
  document: () => string | null;
};

/**
 * Explorer list: the row's folder (empty space and the header: the project folder). The page of the open document,
 * reading or editing: the document's folder. Anything else (top bar, panels, status bar): the project folder.
 */
export function spotAt(x: number, y: number, env: SpotEnv): Spot {
  const at = env.at ?? ((px: number, py: number) => document.elementFromPoint(px, py));
  const element = at(x, y);
  if (element?.closest("[data-drop-explorer]")) {
    return { folder: env.treeFolder(x, y) ?? "", editor: false };
  }
  if (element?.closest("[data-drop-page]")) {
    const open = env.document();
    return {
      folder: open === null ? "" : parentOf(open),
      editor: element.closest("[data-drop-editor]") !== null,
    };
  }
  return { folder: "", editor: false };
}
