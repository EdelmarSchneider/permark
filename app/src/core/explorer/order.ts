// The Explorer's list in name order (`inNameOrder`), kept through deltas: a delta re-places only the entries it brings
// or takes, in the folders they belong to, instead of sorting the whole list again (~70 ms per change at 100k
// entries). The answer always equals a full sort of the list; a delta it cannot follow exactly (a path twice, a node
// `applyDelta` drops, a list it was not built from) is sorted in full.

import type { TreeDelta, TreeNode } from "../../api";
import { flatten, nameCompare, nameGroups, parentOf } from "./list";

type Compare = (a: TreeNode, b: TreeNode) => number;

export class NameOrder {
  /** The list the kept order stands for, and the language it is sorted for. */
  #nodes: readonly TreeNode[] | null = null;
  #locale = "";
  #list: readonly TreeNode[] = [];
  #compare: Compare = () => 0;
  #inFolder = new Map<string, TreeNode[]>();
  #paths = new Set<string>();
  #roots: string[] = [];
  /** No path is listed twice (a list with one is always sorted in full: the steps keep each path once). */
  #regular = true;

  /** `inNameOrder(nodes, locale)`. */
  of(nodes: readonly TreeNode[], locale: string): readonly TreeNode[] {
    this.#sync(nodes, locale);
    return this.#list;
  }

  /** The paths of `nodes`. */
  paths(nodes: readonly TreeNode[], locale: string): ReadonlySet<string> {
    this.#sync(nodes, locale);
    return this.#paths;
  }

  /** The list became `after` = `applyDelta(before, delta)`. */
  step(
    before: readonly TreeNode[],
    delta: TreeDelta,
    after: readonly TreeNode[],
    locale: string,
  ): void {
    const kept = before === this.#nodes && locale === this.#locale && this.#regular;
    if (!kept || !this.#follow(delta, after)) this.#build(after, locale);
  }

  #sync(nodes: readonly TreeNode[], locale: string): void {
    if (nodes !== this.#nodes || locale !== this.#locale) this.#build(nodes, locale);
  }

  #build(nodes: readonly TreeNode[], locale: string): void {
    this.#compare = nameCompare(locale);
    const groups = nameGroups(nodes, this.#compare);
    this.#inFolder = groups.inFolder;
    this.#paths = groups.paths;
    this.#roots = groups.roots;
    this.#regular = groups.paths.size === nodes.length;
    this.#list = flatten(groups.inFolder, groups.roots);
    this.#nodes = nodes;
    this.#locale = locale;
  }

  /** Applies the delta to the kept order; `false`: it cannot be followed exactly (the caller sorts in full; what was
   *  changed by then is rebuilt). */
  #follow(delta: TreeDelta, after: readonly TreeNode[]): boolean {
    const gone = new Set(delta.removed.filter((p) => this.#paths.has(p)));
    const brought = this.#brought(delta, gone);
    if (brought === null) return false;
    if (gone.size === 0 && brought.length === 0) {
      this.#nodes = after;
      return true;
    }
    const wasRoot = new Set(this.#roots);
    const touched = new Set<string>();
    for (const path of gone) touched.add(parentOf(path));
    for (const folder of touched) {
      const kept = (this.#inFolder.get(folder) ?? []).filter((n) => !gone.has(n.path));
      if (kept.length > 0) this.#inFolder.set(folder, kept);
      else this.#inFolder.delete(folder);
    }
    const into = new Map<string, TreeNode[]>();
    for (const node of brought) {
      const folder = parentOf(node.path);
      const list = into.get(folder);
      if (list) list.push(node);
      else into.set(folder, [node]);
    }
    for (const [folder, nodes] of into) {
      const merged = merge(this.#inFolder.get(folder) ?? [], nodes, this.#compare);
      if (merged === null) return false;
      this.#inFolder.set(folder, merged);
    }
    for (const path of gone) this.#paths.delete(path);
    for (const node of brought) this.#paths.add(node.path);
    this.#placeRoots(
      wasRoot,
      [...touched, ...gone, ...into.keys(), ...brought.map((n) => n.path)],
      gone,
      into,
      after,
    );
    this.#list = flatten(this.#inFolder, this.#roots);
    this.#nodes = after;
    return true;
  }

  /** The nodes `applyDelta` places: each after a node that stays, or after another one placed (`null`: first).
   *  `null` when that is not every added node, or a path would be listed twice. */
  #brought(delta: TreeDelta, gone: ReadonlySet<string>): TreeNode[] | null {
    const next = new Map<string | null, TreeNode>();
    for (const { node, after } of delta.added) {
      // `applyDelta` keeps only the last node after one predecessor
      if (next.has(after)) return null;
      next.set(after, node);
    }
    const stays = (path: string): boolean => this.#paths.has(path) && !gone.has(path);
    const out: TreeNode[] = [];
    const seen = new Set<string>();
    for (const start of next.keys()) {
      if (start !== null && !stays(start)) continue;
      for (let node = next.get(start); node !== undefined; node = next.get(node.path)) {
        if (seen.has(node.path) || stays(node.path)) return null;
        seen.add(node.path);
        out.push(node);
      }
    }
    return out.length === delta.added.length ? out : null;
  }

  /** The roots after a delta: folders that hold entries and that no node stands for, in the order their first entry
   *  comes in `after`. Only folders the delta touched can start or stop being a root. The order is read again from
   *  `after` only when it can change: a root came or went, a root other than the first gained an entry (its first
   *  one may now come earlier), or a root other than the last lost one (its first one may now come later). */
  #placeRoots(
    wasRoot: ReadonlySet<string>,
    candidates: readonly string[],
    gone: ReadonlySet<string>,
    into: ReadonlyMap<string, readonly TreeNode[]>,
    after: readonly TreeNode[],
  ): void {
    const roots = new Set(this.#roots);
    let changed = false;
    for (const folder of candidates) {
      const isRoot = (this.#inFolder.get(folder)?.length ?? 0) > 0 && !this.#paths.has(folder);
      if (isRoot === wasRoot.has(folder)) continue;
      changed = true;
      if (isRoot) roots.add(folder);
      else roots.delete(folder);
    }
    const first = this.#roots[0];
    const last = this.#roots[this.#roots.length - 1];
    const moves =
      changed ||
      (this.#roots.length > 1 &&
        (Array.from(into.keys()).some((f) => f !== first && wasRoot.has(f)) ||
          Array.from(gone).some((p) => {
            const f = parentOf(p);
            return f !== last && wasRoot.has(f);
          })));
    if (!moves) return;
    // a pass over the list without a string made per entry: the roots are few
    const pending = Array.from(roots);
    const order: string[] = [];
    for (const node of after) {
      if (pending.length === 0) break;
      const at = node.path.lastIndexOf("/");
      const found = pending.findIndex((root) =>
        root === "" ? at <= 0 : at === root.length && node.path.startsWith(root),
      );
      if (found >= 0) order.push(...pending.splice(found, 1));
    }
    this.#roots = order;
  }
}

/** `list` (ordered) with `added` placed in order; `null` when an added node ties with another entry (the full sort
 *  keeps such entries in list order, which this cannot see). */
function merge(list: readonly TreeNode[], added: TreeNode[], compare: Compare): TreeNode[] | null {
  added.sort(compare);
  for (let i = 1; i < added.length; i++) {
    const a = added[i - 1];
    const b = added[i];
    if (a !== undefined && b !== undefined && compare(a, b) === 0) return null;
  }
  const out: TreeNode[] = [];
  let from = 0;
  for (const node of added) {
    // the first entry not before `node`
    let low = from;
    let high = list.length;
    while (low < high) {
      const mid = (low + high) >>> 1;
      const entry = list[mid];
      if (entry !== undefined && compare(entry, node) < 0) low = mid + 1;
      else high = mid;
    }
    const at = list[low];
    if (at !== undefined && compare(at, node) === 0) return null;
    for (let i = from; i < low; i++) {
      const entry = list[i];
      if (entry !== undefined) out.push(entry);
    }
    out.push(node);
    from = low;
  }
  for (let i = from; i < list.length; i++) {
    const entry = list[i];
    if (entry !== undefined) out.push(entry);
  }
  return out;
}
