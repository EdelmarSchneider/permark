// Unread markers for the mock backend: a set of paths kept in memory. Agents saving a document add it, reading and
// the person's own saves remove it, moves carry it along, and every change is announced the way the real backend does.

import { emitMock } from "../api";
import { isInside } from "../fileOps";
import type { Move } from "../types";
import { docExists, mockDocKind, mockState } from "./fixtures";

/** What changed since the last visit in the mock project, as `mockSince` lists it. */
const unread = new Set<string>([
  "docs/specs/watcher.md",
  "docs/research/storage.md",
  "docs/research/watchers.md",
  "plan.md",
  "@wt/feature-ui/docs/ui-notes.md",
]);

const isDoc = (path: string): boolean => mockDocKind(path) === "markdown";

/** The unread documents that exist, sorted; none in Simple mode. */
export function listUnread(): string[] {
  if (mockState.vault === "simple") return [];
  return [...unread].filter((path) => docExists(path)).sort();
}

function announce(): void {
  setTimeout(() => emitMock("unread-changed", listUnread()), 0);
}

/** Runs a change of the set and announces it when it did something. */
function change(run: () => boolean): void {
  if (run()) announce();
}

const remove = (path: string): boolean => unread.delete(path);
const add = (path: string): boolean => {
  if (!isDoc(path) || unread.has(path)) return false;
  unread.add(path);
  return true;
};

export const markRead = (path: string): void => change(() => remove(path));
export const markUnread = (path: string): void => change(() => add(path));
export const markAllRead = (): void =>
  change(() => {
    const had = unread.size > 0;
    unread.clear();
    return had;
  });
export const markFolderRead = (folder: string): void =>
  change(() => {
    let any = false;
    for (const path of [...unread]) {
      if (folder === "" || isInside(path, folder)) any = unread.delete(path) || any;
    }
    return any;
  });

/** An agent saved or created a document. */
export const noteChanged = (path: string): void => change(() => add(path));

/** A document is gone (a folder with documents in it too). */
export const noteGone = (path: string): void =>
  change(() => {
    let any = false;
    for (const doc of [...unread]) {
      if (isInside(doc, path)) any = unread.delete(doc) || any;
    }
    return any;
  });

/** The person moved files or folders in the Explorer: what was unread stays unread, where it is now. */
export const noteMoved = (moves: readonly Move[]): void =>
  change(() => {
    let any = false;
    for (const move of moves) {
      for (const doc of [...unread]) {
        if (!isInside(doc, move.from)) continue;
        unread.delete(doc);
        unread.add(move.to + doc.slice(move.from.length));
        any = true;
      }
    }
    return any;
  });
