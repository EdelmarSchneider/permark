// Which paths are silenced. Everything speaks until a mark says otherwise. The closest mark wins,
// and it covers the path, what is inside it, and what appears there later.

import { ancestors, splitWorktree } from "./paths";
import type { FsBatch } from "./types";

export type Interest = "quiet" | "speaking";

/** `path`, then each folder above it, closest first, then the project root when `path` is in it. */
export function interestChain(path: string): string[] {
  const chain = [path];
  const above = ancestors(path);
  for (let i = above.length - 1; i >= 0; i--) chain.push(above[i]);
  if (splitWorktree(path).worktree === null && path !== "") chain.push("");
  return chain;
}

/** True when the closest mark on `path` or a folder above it is quiet. */
export function isQuietPath(path: string, marks: ReadonlyMap<string, Interest>): boolean {
  for (const at of interestChain(path)) {
    const mark = marks.get(at);
    if (mark !== undefined) return mark === "quiet";
  }
  return false;
}

/**
 * The batch with silenced paths taken out. A bulk batch whose listed paths are all quiet is dropped.
 * A mixed bulk stays whole: the row is one event.
 */
export function withoutQuiet(batch: FsBatch, quietPath: (path: string) => boolean): FsBatch {
  if (batch.bulk !== null) {
    const listed = batch.bulk.paths;
    if (listed.length > 0 && listed.every(quietPath)) {
      return { ...batch, bulk: null, changes: [] };
    }
    return batch;
  }
  const changes = batch.changes.filter(
    (change) => !quietPath(change.path) && (change.from === undefined || !quietPath(change.from)),
  );
  return changes.length === batch.changes.length ? batch : { ...batch, changes };
}
