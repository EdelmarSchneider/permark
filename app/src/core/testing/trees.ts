// Made-up Explorer lists for tests and benches that need sizes no mock project has. Lists come in the backend's
// order: the project's entries first, then each worktree's; within one, a folder is followed by what it holds and
// names follow code points.

import type { NodeKind, TreeNode } from "../../api";
import { nameOf, rootOf } from "../explorer/list";

export const FILE: NodeKind = { type: "file", category: "markdown" };
export const DIR: NodeKind = { type: "dir", unlisted: null };

export function treeNode(path: string, kind: NodeKind = FILE): TreeNode {
  return { path, name: nameOf(path), kind, hidden: false, ignored: false, forced: false };
}

/** Small deterministic random (tests repeat). */
export function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

/** The backend's order of any set of nodes (see the top of this file). */
export function backendOrder(nodes: readonly TreeNode[]): TreeNode[] {
  const keyed = nodes.map((node) => ({
    node,
    worktree: rootOf(node.path) !== "",
    parts: node.path.split("/"),
  }));
  keyed.sort((a, b) => {
    if (a.worktree !== b.worktree) return a.worktree ? 1 : -1;
    const n = Math.min(a.parts.length, b.parts.length);
    for (let i = 0; i < n; i++) {
      const x = a.parts[i] ?? "";
      const y = b.parts[i] ?? "";
      if (x !== y) return x < y ? -1 : 1;
    }
    return a.parts.length - b.parts.length;
  });
  return keyed.map((k) => k.node);
}

/** Names that make the language's order differ from code points: case, numbers, accents. */
const STEMS = ["note", "Note", "file", "Éclair", "zeta", "Alpha", "beta", "émoi"];

/** About `files` files in folders of up to `perFolder` entries, nested a few levels; `worktrees` (names) hold a
 *  smaller copy each, listed after the project. */
export function bigList(
  files: number,
  options: { perFolder?: number; worktrees?: readonly string[]; seed?: number } = {},
): TreeNode[] {
  const perFolder = options.perFolder ?? 40;
  const random = rng(options.seed ?? 1);
  const out: TreeNode[] = [];
  const fill = (root: string, count: number): void => {
    const folders: string[] = [root];
    let made = 0;
    for (let at = 0; made < count && at < folders.length; at++) {
      const folder = folders[at] ?? root;
      const prefix = folder === "" ? "" : `${folder}/`;
      for (let i = 0; i < perFolder && made < count; i++) {
        const stem = STEMS[Math.floor(random() * STEMS.length)] ?? "note";
        const number = Math.floor(random() * 1000);
        if (random() < 0.12) {
          const path = `${prefix}${stem} dir ${number}-${made}`;
          out.push(treeNode(path, DIR));
          folders.push(path);
        } else {
          out.push(treeNode(`${prefix}${stem} ${number}-${made}.md`));
          made++;
        }
      }
    }
  };
  fill("", files);
  for (const name of options.worktrees ?? []) fill(`@wt/${name}`, Math.ceil(files / 10));
  return backendOrder(out);
}
