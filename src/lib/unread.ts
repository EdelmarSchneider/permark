// Pure helpers of the unread markers: how many are unread inside each folder, the tree reduced to what is unread, and
// which document "next unread" goes to. The set itself comes from the backend (`stores/unread.svelte.ts`).

import { ancestors } from "./paths";
import { isDirNode } from "./tree";
import type { FsBatch, TreeNode } from "./types";

/** Unread documents inside each folder (a document counts for every folder above it, worktree roots included). */
export function folderCounts(unread: Iterable<string>): Map<string, number> {
  const counts = new Map<string, number>();
  for (const path of unread) {
    for (const folder of ancestors(path)) counts.set(folder, (counts.get(folder) ?? 0) + 1);
  }
  return counts;
}

/**
 * The tree with only the unread documents and the folders on the way to them. `keep` (the open document) stays even
 * when it is read, so a document does not vanish from under the person's eyes the moment it counts as read.
 */
export function onlyUnread(
  nodes: readonly TreeNode[],
  unread: ReadonlySet<string>,
  keep: string | null,
): TreeNode[] {
  const out: TreeNode[] = [];
  for (const node of nodes) {
    if (isDirNode(node)) {
      const children = onlyUnread(node.children ?? [], unread, keep);
      if (children.length > 0) out.push({ ...node, children });
    } else if (unread.has(node.path) || node.path === keep) {
      out.push(node);
    }
  }
  return out;
}

export type NextUnreadInput = {
  unread: ReadonlySet<string>;
  /** The documents that changed while the project was open, newest first (the live feed). */
  feed: readonly string[];
  /** Every file of the tree, in tree order. */
  tree: readonly string[];
  /** The document on show. */
  current: string | null;
};

/** The unread documents in the order "next unread" walks them: the live feed, newest first, then the tree. */
export function unreadWalk({ unread, feed, tree }: Omit<NextUnreadInput, "current">): string[] {
  const walk: string[] = [];
  const seen = new Set<string>();
  const add = (path: string): void => {
    if (unread.has(path) && !seen.has(path)) {
      seen.add(path);
      walk.push(path);
    }
  };
  feed.forEach(add);
  tree.forEach(add);
  // unread documents the tree does not list (it is behind, or the Explorer filter keeps them out)
  [...unread].sort().forEach(add);
  return walk;
}

/**
 * The document after the one on show in the walk, wrapping round; the first one when the document on show is read.
 * Null when nothing else is unread.
 */
export function nextUnread(input: NextUnreadInput): string | null {
  const walk = unreadWalk(input);
  const at = input.current === null ? -1 : walk.indexOf(input.current);
  for (let step = 1; step <= walk.length; step++) {
    const path = walk[(at + step + walk.length) % walk.length];
    if (path !== input.current) return path;
  }
  return null;
}

/** Whether a batch of changes touches the file (a bulk batch lists some of its paths; lost events could mean anything). */
export function batchTouches(batch: FsBatch, path: string): boolean {
  if (batch.rescan) return true;
  if (batch.changes.some((c) => c.path === path || c.from === path)) return true;
  return batch.bulk?.paths.includes(path) ?? false;
}
