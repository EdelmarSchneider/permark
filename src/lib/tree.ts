// Pure tree helpers: flattening for rendering, git roll-up dots, badges and keyboard navigation.

import { ancestors } from "./paths";
import type { GitSnapshot, GitState, TreeNode } from "./types";

export type FlatRow = {
  node: TreeNode;
  depth: number;
  isDir: boolean;
  expanded: boolean;
};

export function isDirNode(node: TreeNode): boolean {
  return node.kind === "dir" || node.kind === "worktree";
}

/** The visible rows of the tree, depth first, honoring the set of expanded folders. */
export function flattenTree(
  nodes: TreeNode[],
  expanded: ReadonlySet<string>,
  depth = 0,
  out: FlatRow[] = [],
): FlatRow[] {
  for (const node of nodes) {
    const isDir = isDirNode(node);
    const open = isDir && expanded.has(node.path);
    out.push({ node, depth, isDir, expanded: open });
    if (open && node.children) flattenTree(node.children, expanded, depth + 1, out);
  }
  return out;
}

/** `listTree` returns the main root; worktrees may be among its children (kind "worktree"). */
export function splitRoots(root: TreeNode): { main: TreeNode[]; worktrees: TreeNode[] } {
  const children = root.children ?? [];
  return {
    main: children.filter((n) => n.kind !== "worktree"),
    worktrees: children.filter((n) => n.kind === "worktree"),
  };
}

/** Every file path in the tree (worktree files included), for quick open. */
export function collectFiles(nodes: TreeNode[], out: string[] = []): string[] {
  for (const node of nodes) {
    if (isDirNode(node)) collectFiles(node.children ?? [], out);
    else out.push(node.path);
  }
  return out;
}

export function findNode(nodes: TreeNode[], path: string): TreeNode | null {
  for (const node of nodes) {
    if (node.path === path) return node;
    const hit = node.children ? findNode(node.children, path) : null;
    if (hit) return hit;
  }
  return null;
}

// ---- git decorations -----------------------------------------------------------------------

export type Rollup = "conflicted" | "deleted" | "modified" | "added";

const ROLLUP_RANK: Record<Rollup, number> = { conflicted: 4, deleted: 3, modified: 2, added: 1 };

function rollupOf(state: GitState): Rollup {
  switch (state) {
    case "conflicted":
      return "conflicted";
    case "deleted":
      return "deleted";
    case "added":
    case "untracked":
      return "added";
    default:
      return "modified";
  }
}

/** Folder path -> worst git state inside it (conflicted > deleted > modified > added). */
export function rollups(files: GitSnapshot["files"]): Map<string, Rollup> {
  const map = new Map<string, Rollup>();
  for (const [path, info] of Object.entries(files)) {
    const level = rollupOf(info.state);
    for (const dir of ancestors(path)) {
      const current = map.get(dir);
      if (!current || ROLLUP_RANK[level] > ROLLUP_RANK[current]) map.set(dir, level);
    }
  }
  return map;
}

export type Badge = { letter: string; className: string; state: GitState };

const BADGES: Record<GitState, Badge> = {
  modified: { letter: "M", className: "badge--M", state: "modified" },
  added: { letter: "A", className: "badge--A", state: "added" },
  untracked: { letter: "U", className: "badge--U", state: "untracked" },
  deleted: { letter: "D", className: "badge--D", state: "deleted" },
  renamed: { letter: "R", className: "badge--R", state: "renamed" },
  conflicted: { letter: "C", className: "badge--C", state: "conflicted" },
};

export function badgeFor(state: GitState | undefined): Badge | null {
  return state ? BADGES[state] : null;
}

// ---- keyboard ------------------------------------------------------------------------------

export type TreeNavResult = {
  /** Row to move the focus to. */
  focus: number;
  /** Folder to expand or collapse. */
  toggle?: { path: string; expand: boolean };
  /** File to open. */
  open?: string;
};

/** What a key press does on the focused row; null when the key is not a tree key. */
export function navigate(rows: FlatRow[], index: number, key: string): TreeNavResult | null {
  if (rows.length === 0) return null;
  const row = rows[index];
  const last = rows.length - 1;
  switch (key) {
    case "ArrowDown":
      return { focus: Math.min(index + 1, last) };
    case "ArrowUp":
      return { focus: Math.max(index - 1, 0) };
    case "Home":
      return { focus: 0 };
    case "End":
      return { focus: last };
    case "ArrowRight":
      if (!row?.isDir) return { focus: index };
      if (!row.expanded) return { focus: index, toggle: { path: row.node.path, expand: true } };
      return { focus: index + 1 <= last && rows[index + 1].depth > row.depth ? index + 1 : index };
    case "ArrowLeft": {
      if (row?.isDir && row.expanded) {
        return { focus: index, toggle: { path: row.node.path, expand: false } };
      }
      for (let i = index - 1; i >= 0; i--) {
        if (rows[i].depth < (row?.depth ?? 0)) return { focus: i };
      }
      return { focus: index };
    }
    case "Enter":
    case " ":
      if (!row) return null;
      if (row.isDir)
        return { focus: index, toggle: { path: row.node.path, expand: !row.expanded } };
      return { focus: index, open: row.node.path };
    default:
      return null;
  }
}

// ---- file operations -----------------------------------------------------------------------

/** The names of the entries directly inside a folder of the tree ("" is the project root), files and folders alike. */
export function childNames(root: TreeNode | null, folder: string): string[] {
  const nodes = root?.children ?? [];
  const children = folder === "" ? nodes : (findNode(nodes, folder)?.children ?? []);
  // the worktrees listed beside the project's own entries are not entries of the project
  return children.filter((n) => folder !== "" || n.kind !== "worktree").map((n) => n.name);
}

/** Whether a path of the tree is a folder (or a worktree). False for files and for paths the tree does not hold. */
export function isDirPath(root: TreeNode | null, path: string): boolean {
  const node = findNode(root?.children ?? [], path);
  return node !== null && isDirNode(node);
}

/** Type-ahead: the next row (after `from`, wrapping) whose name starts with `prefix`. */
export function typeahead(rows: FlatRow[], from: number, prefix: string): number | null {
  const p = prefix.toLowerCase();
  if (p === "" || rows.length === 0) return null;
  for (let step = 0; step < rows.length; step++) {
    const i = (from + (prefix.length > 1 ? 0 : 1) + step) % rows.length;
    if (rows[i].node.name.toLowerCase().startsWith(p)) return i;
  }
  return null;
}
