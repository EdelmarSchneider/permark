// Pure logic of the Explorer list: applying deltas, rows from the flat list, selection ranges, drop rules, UI state
// that follows renames. No state, no DOM: the store and the components call these.

import type { Added, RootNode, TreeDelta, TreeNode } from "../../api";

const WORKTREE = "@wt/";

/** `a/b/c.md` -> `c.md`. */
export function nameOf(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** Folder of a project path: `""` for a top-level entry; `@wt/x` holds the top level of worktree `x`. */
export function parentOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/** `@wt/x` for a path inside worktree `x`, `""` for the project folder. */
export function rootOf(path: string): string {
  if (!path.startsWith(WORKTREE)) return "";
  const end = path.indexOf("/", WORKTREE.length);
  return end < 0 ? path : path.slice(0, end);
}

/** Path below its root: `@wt/x/a/b` -> `a/b`. */
export function belowRoot(path: string): string {
  const root = rootOf(path);
  return root === "" ? path : path.slice(root.length + 1);
}

/** Folders above a path, outermost first, root prefix included for a worktree (`@wt/x`, `@wt/x/a`). */
export function ancestorsOf(path: string): string[] {
  const out: string[] = [];
  let at = parentOf(path);
  while (at !== "") {
    out.push(at);
    at = parentOf(at);
  }
  // `@wt` alone is no folder
  return out.filter((p) => p !== "@wt").reverse();
}

export function isWithin(path: string, folder: string): boolean {
  return folder === "" || path === folder || path.startsWith(`${folder}/`);
}

/** Depth of a node below its root (top level = 0). */
export function depthOf(path: string): number {
  const rel = belowRoot(path);
  return rel === "" ? 0 : rel.split("/").length - 1;
}

// ---- deltas --------------------------------------------------------------------------------------------------------

/** The list after a delta: drop `removed`, then each added node after its predecessor. One pass. */
export function applyDelta(nodes: readonly TreeNode[], delta: TreeDelta): TreeNode[] {
  const gone = new Set(delta.removed);
  const after = new Map<string | null, TreeNode>();
  for (const added of delta.added) after.set(added.after, added.node);
  const out: TreeNode[] = [];
  const append = (from: string | null): void => {
    // each node has at most one added successor; a chain of added nodes follows its first
    let next = after.get(from);
    while (next !== undefined) {
      out.push(next);
      next = after.get(next.path);
    }
  };
  append(null);
  for (const node of nodes) {
    if (gone.has(node.path)) continue;
    out.push(node);
    append(node.path);
  }
  return out;
}

function sameNode(a: TreeNode, b: TreeNode): boolean {
  return (
    a.path === b.path &&
    a.name === b.name &&
    a.hidden === b.hidden &&
    a.ignored === b.ignored &&
    a.forced === b.forced &&
    JSON.stringify(a.kind) === JSON.stringify(b.kind)
  );
}

/** Difference of two lists as a delta body (what the backend sends; the mock backend uses it). */
export function diffLists(
  before: readonly TreeNode[],
  after: readonly TreeNode[],
): { removed: string[]; added: Added[] } {
  const was = new Map(before.map((n) => [n.path, n] as const));
  const is = new Map(after.map((n) => [n.path, n] as const));
  const removed = before
    .filter((n) => {
      const now = is.get(n.path);
      return now === undefined || !sameNode(now, n);
    })
    .map((n) => n.path);
  const added: Added[] = [];
  after.forEach((node, at) => {
    const old = was.get(node.path);
    if (old === undefined || !sameNode(old, node)) {
      added.push({ node, after: at === 0 ? null : (after[at - 1]?.path ?? null) });
    }
  });
  return { removed, added };
}

/** A path after folder renames (`a` -> `b` moves `a/x` to `b/x`). */
export function renamedPath(
  path: string,
  renames: readonly { from: string; to: string }[],
): string {
  for (const { from, to } of renames) {
    if (path === from) return to;
    if (path.startsWith(`${from}/`)) return to + path.slice(from.length);
  }
  return path;
}

/** UI state keyed by path (selection, expansion) follows renames. */
export function remapSet(
  paths: ReadonlySet<string>,
  renames: readonly { from: string; to: string }[],
): Set<string> {
  if (renames.length === 0) return new Set(paths);
  return new Set(Array.from(paths, (p) => renamedPath(p, renames)));
}

// ---- rows ------------------------------------------------------------------------------------------------------------

export type Row =
  | { type: "root"; root: RootNode; expanded: boolean }
  | { type: "node"; node: TreeNode; depth: number; expanded: boolean };

export function rowKey(row: Row): string {
  return row.type === "root" ? row.root.path : row.node.path;
}

/**
 * The visible rows: a node shows when every folder above it is open. Worktrees get a header row of their own (open
 * unless listed in `collapsedRoots`). `expanded` holds the open folders.
 */
export function rowsOf(
  roots: readonly RootNode[],
  nodes: readonly TreeNode[],
  expanded: ReadonlySet<string>,
  collapsedRoots: ReadonlySet<string>,
): Row[] {
  const rows: Row[] = [];
  const stack: { path: string; open: boolean }[] = [];
  const worktrees = roots.filter((r) => r.kind === "worktree");
  const header = new Map(worktrees.map((r) => [r.path, r] as const));
  let current = "";
  let rootOpen = true;
  for (const node of nodes) {
    const root = rootOf(node.path);
    if (root !== current) {
      current = root;
      stack.length = 0;
      const rootNode = header.get(root);
      if (rootNode) {
        rootOpen = !collapsedRoots.has(root);
        rows.push({ type: "root", root: rootNode, expanded: rootOpen });
      } else {
        rootOpen = true;
      }
    }
    while (stack.length > 0) {
      const top = stack[stack.length - 1];
      if (top && node.path.startsWith(`${top.path}/`)) break;
      stack.pop();
    }
    const visible = rootOpen && stack.every((s) => s.open);
    const isDir = node.kind.type === "dir";
    const open = isDir && expanded.has(node.path);
    if (visible) rows.push({ type: "node", node, depth: depthOf(node.path), expanded: open });
    if (isDir) stack.push({ path: node.path, open });
  }
  return rows;
}

// ---- selection -------------------------------------------------------------------------------------------------------

/** Keys of the rows between two keys (inclusive, either order). Unknown keys -> just `to`. */
export function rangeBetween(rows: readonly Row[], from: string, to: string): string[] {
  const keys = rows.filter((r) => r.type === "node").map(rowKey);
  const a = keys.indexOf(from);
  const b = keys.indexOf(to);
  if (a < 0 || b < 0) return [to];
  return keys.slice(Math.min(a, b), Math.max(a, b) + 1);
}

/** Selected paths without those inside another selected folder (a move or delete takes the folder along). */
export function topmost(paths: Iterable<string>): string[] {
  const all = Array.from(new Set(paths));
  return all.filter((p) => !all.some((other) => other !== p && p.startsWith(`${other}/`)));
}

// ---- drag and drop ---------------------------------------------------------------------------------------------------

/** Where a dragged set may land: a folder (or the project root `""`) that is not inside what moves, not where it
 * already is, in the same root. */
export function canDrop(dragged: readonly string[], target: string): boolean {
  if (dragged.length === 0) return false;
  const targetRoot = rootOf(target);
  for (const path of dragged) {
    if (rootOf(path) !== targetRoot) return false;
    if (target === path || target.startsWith(`${path}/`)) return false;
  }
  return dragged.some((path) => parentOf(path) !== target);
}

/** The move specs of dragging `paths` into `target` (names kept). */
export function movesInto(
  paths: readonly string[],
  target: string,
): { from: string; to: string }[] {
  return paths.map((from) => ({
    from,
    to: target === "" ? nameOf(from) : `${target}/${nameOf(from)}`,
  }));
}

/** Name of a path with its extension split off: what an inline rename selects. */
export function stemLength(name: string): number {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? dot : name.length;
}
