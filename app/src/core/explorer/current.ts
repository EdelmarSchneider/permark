// The Explorer of the project this window shows, for the parts of the app that point at the tree (a document's header
// "reveal in tree", a link to a folder, the commands). Set by the sidebar while it lives.

import type { NewKind } from "../../api";

type Handle = {
  reveal(path: string): void;
  revealActive(): void;
  beginNew(kind: NewKind): void;
  copyPath(path: string, full: boolean): Promise<void>;
  /** Takes the newest file operation back. */
  undo(): Promise<void>;
  /** The folder files dropped from outside at this point land in; `null`: the point is not over the list. */
  dropFolderAt(x: number, y: number): string | null;
  /** Marks the folder an outside drag is over (`null`: clear). */
  markDrop(folder: string | null): void;
};

let current: Handle | null = null;

export function setExplorer(explorer: Handle | null): void {
  current = explorer;
}

export function revealInTree(path: string | null): void {
  if (path === null) current?.revealActive();
  else current?.reveal(path);
}

/** Starts a new file | folder in the Explorer (the sidebar shows the Files view first). */
export function newInTree(kind: NewKind): void {
  current?.beginNew(kind);
}

export function copyPathOf(path: string, full: boolean): void {
  void current?.copyPath(path, full);
}

/** Folder of the project tree under a point of the window (`""` = the project folder); `null`: not over the tree. */
export function treeFolderAt(x: number, y: number): string | null {
  return current?.dropFolderAt(x, y) ?? null;
}

/** The tree marks the folder files dragged in from outside would land in (`null`: no mark). */
export function markTreeDrop(folder: string | null): void {
  current?.markDrop(folder);
}

/** Takes the newest file operation back (the notice of an import offers it). */
export async function undoInTree(): Promise<void> {
  await current?.undo();
}
