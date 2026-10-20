// The Explorer of the project this window shows, for the parts of the app that point at the tree (a document's header
// "reveal in tree", a link to a folder, the commands). Set by the sidebar while it lives.

import { createSubscriber } from "svelte/reactivity";
import type { NewKind } from "../../api";

type Handle = {
  /** The folders open in the tree (reactive). */
  readonly expanded: ReadonlySet<string>;
  /** The one folder selected in the tree (`null`: none, or more than one thing). */
  selectedFolder(): string | null;
  /** The tree's filter shows hidden files (dot files | folders). */
  showsHidden(): boolean;
  reveal(path: string): void;
  revealActive(): void;
  beginNew(kind: NewKind): void;
  copyPath(path: string, full: boolean): Promise<void>;
  /** Takes the newest file operation back. */
  undo(): Promise<void>;
  /** Takes back exactly the file operation `id`. */
  undoEntry(id: number): Promise<void>;
  /** The folder files dropped from outside at this point land in; `null`: the point is not over the list. */
  dropFolderAt(x: number, y: number): string | null;
  /** An outside drag is over a folder of the list (`null`: none) at a point (`null`: not over the list). */
  outsideOver(folder: string | null, point: { x: number; y: number } | null): void;
  /** Dropped on the list: the mark goes, folders the drag opened stay while the question is open. */
  outsideHold(): void;
  /** The drag is over: folders it opened close, except the one an import landed in (`kept`) and those above it. */
  outsideEnd(kept: string | null): void;
  /** Items an import put in place: selected, the first brought into view. */
  placed(paths: readonly string[]): void;
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

/** Whether this window's tree shows hidden files (`false` with no tree). */
export function explorerShowsHidden(): boolean {
  return current?.showsHidden() ?? false;
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

/** The tree that shows an outside drag over it, and what became of the drop. */
export const treeDrop = {
  over: (folder: string | null, point: { x: number; y: number } | null): void =>
    current?.outsideOver(folder, point),
  hold: (): void => current?.outsideHold(),
  end: (kept: string | null): void => current?.outsideEnd(kept),
  placed: (paths: readonly string[]): void => current?.placed(paths),
};

/** Takes the newest file operation back. */
export async function undoInTree(): Promise<void> {
  await current?.undo();
}

/** Takes back exactly the file operation `id` (the notice of an import offers it). */
export async function undoEntryInTree(id: number): Promise<void> {
  await current?.undoEntry(id);
}
