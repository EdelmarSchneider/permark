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

// ---- order -----------------------------------------------------------------------------------------------------------

/**
 * The list as the person reads it: in each folder, folders first, then names as `locale` sorts them, numbers by their
 * value ("file 2" before "file 10"), as the system's file manager does. Names that language sees as equal ("a", "A")
 * keep one fixed order (code points). The backend's order (code points) stays what deltas are placed by; roots keep
 * theirs.
 */
export function inNameOrder(nodes: readonly TreeNode[], locale: string): TreeNode[] {
  const groups = nameGroups(nodes, nameCompare(locale));
  return flatten(groups.inFolder, groups.roots);
}

/** The order of the entries of one folder (`inNameOrder`). Makes one collator: callers keep it for a whole list. */
export function nameCompare(locale: string): (a: TreeNode, b: TreeNode) => number {
  const collator = new Intl.Collator(locale, { numeric: true, sensitivity: "base" });
  return (a, b) =>
    Number(a.kind.type !== "dir") - Number(b.kind.type !== "dir") ||
    collator.compare(a.name, b.name) ||
    (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
}

/** What the name order is made of: each folder's entries in order, the paths listed, and the roots (`""`,
 *  `@wt/<name>`): folders no node stands for, in the order their first entry comes. */
export type NameGroups = {
  inFolder: Map<string, TreeNode[]>;
  paths: Set<string>;
  roots: string[];
};

export function nameGroups(
  nodes: readonly TreeNode[],
  compare: (a: TreeNode, b: TreeNode) => number,
): NameGroups {
  const inFolder = new Map<string, TreeNode[]>();
  for (const node of nodes) {
    const folder = parentOf(node.path);
    const list = inFolder.get(folder);
    if (list) list.push(node);
    else inFolder.set(folder, [node]);
  }
  for (const list of inFolder.values()) list.sort(compare);
  const paths = new Set(nodes.map((n) => n.path));
  const roots = Array.from(inFolder.keys()).filter((folder) => !paths.has(folder));
  return { inFolder, paths, roots };
}

/** The list from each folder's ordered entries: root after root, a folder's entries right after it. Entries under
 *  a node that is no folder stay out, as do folders no root leads to. */
export function flatten(
  inFolder: ReadonlyMap<string, readonly TreeNode[]>,
  roots: readonly string[],
): TreeNode[] {
  const out: TreeNode[] = [];
  const visit = (folder: string): void => {
    for (const node of inFolder.get(folder) ?? []) {
      out.push(node);
      if (node.kind.type === "dir") visit(node.path);
    }
  };
  for (const root of roots) visit(root);
  return out;
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

/** Selected paths without those inside another selected folder (a move or delete takes the folder along). Each path
 *  looks up only its own ancestors (every `/` boundary, worktree roots included): 20k selected rows took seconds when
 *  every path was checked against every other one. */
export function topmost(paths: Iterable<string>): string[] {
  const all = new Set(paths);
  return Array.from(all).filter((p) => {
    for (let at = p.lastIndexOf("/"); at >= 0; at = at === 0 ? -1 : p.lastIndexOf("/", at - 1)) {
      if (all.has(p.slice(0, at))) return false;
    }
    return true;
  });
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

/** The edge zone (px) of the list where a drag scrolls it. */
export const EDGE_ZONE = 28;
const EDGE_SLOW = 2;
const EDGE_FAST = 14;

/** Pixels per frame a drag at height `y` scrolls a list spanning `top`..`bottom`: negative up, positive down, 0
 *  outside the edge zones (or outside the list). Faster closer to the edge. */
export function edgeSpeed(y: number, top: number, bottom: number): number {
  if (!(y >= top && y <= bottom) || bottom - top <= 0) return 0;
  const zone = Math.min(EDGE_ZONE, (bottom - top) / 2);
  const speed = (distance: number): number =>
    Math.round(EDGE_SLOW + (EDGE_FAST - EDGE_SLOW) * (1 - distance / zone));
  if (y < top + zone) return -speed(y - top);
  if (y > bottom - zone) return speed(bottom - y);
  return 0;
}

/** The rows that read as one block with an outside drop's target folder: its visible descendants (the rows right
 *  below it, deeper than it; for a worktree root, its rows). `null`: no target row, or nothing below it. */
export function dropBlock(
  rows: readonly Row[],
  target: string | null,
): { first: number; last: number } | null {
  if (target === null || target === "") return null;
  const at = rows.findIndex((r) => rowKey(r) === target);
  const row = rows[at];
  if (row === undefined) return null;
  let last = at;
  for (let i = at + 1; i < rows.length; i += 1) {
    const next = rows[i];
    if (next === undefined || next.type === "root") break;
    if (row.type === "node" && next.depth <= row.depth) break;
    last = i;
  }
  return last === at ? null : { first: at + 1, last };
}
