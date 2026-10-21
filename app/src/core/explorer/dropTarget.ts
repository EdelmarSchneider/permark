// What is under the pointer of a drag from outside: the part of the view there says so.

import type { DropSpot } from "./importModel";

export type SpotEnv = {
  /** The element under a point (the document's by default). */
  at?: (x: number, y: number) => Element | null;
  /** The tree's folder for a point over its list (`null`: not over the list, e.g. its header). */
  treeFolder: (x: number, y: number) => string | null;
  /** The editor on screen takes pictures. */
  editorTakesPictures: () => boolean;
};

/** The Explorer list (with the folder a drop lands in), an editor in Edit | Source, or anything else. */
export function spotAt(x: number, y: number, env: SpotEnv): DropSpot {
  const at = env.at ?? ((px: number, py: number) => document.elementFromPoint(px, py));
  const element = at(x, y);
  if (element?.closest("[data-drop-explorer]")) {
    const folder = env.treeFolder(x, y);
    if (folder !== null) return { area: "explorer", folder };
  }
  if (element?.closest("[data-drop-editor]")) {
    return { area: "editor", pictures: env.editorTakesPictures() };
  }
  return { area: "other" };
}
