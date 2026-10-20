// The Explorer of the project this window shows, for the parts of the app that point at the tree (a document's header
// "reveal in tree", a link to a folder, the commands). Set by the sidebar while it lives.

import { createSubscriber } from "svelte/reactivity";
import type { NewKind } from "../../api";

type Handle = {
  /** The folders open in the tree (reactive). */
  readonly expanded: ReadonlySet<string>;
  /** The one folder selected in the tree (`null`: none, or more than one thing). */
  selectedFolder(): string | null;
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
/** Tells the effects that read the handle that the tree came or went. */
let tell: (() => void) | null = null;
const watching = createSubscriber((update) => {
  tell = update;
  return () => {
    tell = null;
  };
});

export function setExplorer(explorer: Handle | null): void {
  current = explorer;
  tell?.();
}

/** The folders open in the tree now (reactive, also to the tree coming and going; empty while no tree is shown). */
export function explorerExpanded(): ReadonlySet<string> {
  watching();
  return current?.expanded ?? NONE;
}

/** The folder the person selected in the tree (`null`: no tree, or not exactly one folder selected). */
export function explorerSelectedFolder(): string | null {
  return current?.selectedFolder() ?? null;
}

const NONE: ReadonlySet<string> = new Set();

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
