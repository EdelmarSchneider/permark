// The Explorer of the project this window shows, for the parts of the app that point at the tree (a document's header
// "reveal in tree", a link to a folder, the commands). Set by the sidebar while it lives.

import type { NewKind } from "../../api";

type Handle = {
  reveal(path: string): void;
  revealActive(): void;
  beginNew(kind: NewKind): void;
  copyPath(path: string, full: boolean): Promise<void>;
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
