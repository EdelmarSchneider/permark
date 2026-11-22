import { afterEach, describe, expect, it, vi } from "vitest";
import type { TreeDelta, TreeNode } from "../../api";
import { DIR, FILE, backendOrder, bigList, rng, treeNode } from "../testing/trees";
import { applyDelta, diffLists, inNameOrder, parentOf } from "./list";
import { NameOrder } from "./order";

/** The name order as it was first written (one full sort per call): what the kept order must always equal. */
function reference(nodes: readonly TreeNode[], locale: string): TreeNode[] {
  const collator = new Intl.Collator(locale, { numeric: true, sensitivity: "base" });
  const compare = (a: TreeNode, b: TreeNode): number =>
    Number(a.kind.type !== "dir") - Number(b.kind.type !== "dir") ||
    collator.compare(a.name, b.name) ||
    (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
  const inFolder = new Map<string, TreeNode[]>();
  for (const node of nodes) {
    const folder = parentOf(node.path);
    const list = inFolder.get(folder);
    if (list) list.push(node);
    else inFolder.set(folder, [node]);
  }
  const listed = new Set(nodes.map((n) => n.path));
  const out: TreeNode[] = [];
  const visit = (folder: string): void => {
    for (const node of (inFolder.get(folder) ?? []).sort(compare)) {
      out.push(node);
      if (node.kind.type === "dir") visit(node.path);
    }
  };
  for (const folder of inFolder.keys()) if (!listed.has(folder)) visit(folder);
  return out;
}

function delta(partial: Partial<TreeDelta>): TreeDelta {
  return {
    seq: 1,
    removed: [],
    added: [],
    renamed: [],
    totalFiles: 0,
    shownFiles: 0,
    bulk: false,
    filter: null,
    ...partial,
  };
}

const paths = (list: readonly TreeNode[]): string[] => list.map((n) => n.path);

/** Full sorts made while `run` runs (each makes one collator; a kept order makes none). */
function fullSorts(run: () => void): number {
  const made = vi.spyOn(Intl, "Collator");
  try {
    run();
    return made.mock.calls.length;
  } finally {
    made.mockRestore();
  }
}

afterEach(() => {
  vi.restoreAllMocks();
});

/** Names whose language order differs from code points, and pairs the language sees as equal. */
const NAMES = [
  "a",
  "A",
  "b",
  "file 2",
  "file 10",
  "File 1",
  "Éclair",
  "eclair",
  "zeta",
  "Ölmal",
  "émoi",
  "_x",
];

/** A random change to a project: files and folders come and go, folders are renamed, entries change kind or
 *  attributes, worktrees appear and go. Returns the list in the backend's order. */
function change(list: readonly TreeNode[], next: () => number): TreeNode[] {
  const byPath = new Map(list.map((n) => [n.path, n] as const));
  const folders = (): string[] => [
    "",
    ...Array.from(byPath.values())
      .filter((n) => n.kind.type === "dir")
      .map((n) => n.path),
  ];
  const pick = <T>(items: readonly T[]): T | undefined => items[Math.floor(next() * items.length)];
  const name = (): string => `${pick(NAMES) ?? "a"}${next() < 0.5 ? ".md" : ""}`;
  const inside = (folder: string, leaf: string): string =>
    folder === "" ? leaf : `${folder}/${leaf}`;
  const dropTree = (path: string): void => {
    for (const p of Array.from(byPath.keys()))
      if (p === path || p.startsWith(`${path}/`)) byPath.delete(p);
  };
  const steps = 1 + Math.floor(next() * 3);
  for (let s = 0; s < steps; s++) {
    const roll = next();
    const all = Array.from(byPath.keys());
    if (roll < 0.25) {
      const path = inside(pick(folders()) ?? "", name());
      if (!byPath.has(path)) byPath.set(path, treeNode(path, next() < 0.3 ? DIR : FILE));
    } else if (roll < 0.4) {
      const path = pick(all);
      if (path !== undefined) dropTree(path);
    } else if (roll < 0.55) {
      // a folder renamed (or moved): its subtree leaves, the same entries arrive under the new name
      const from = pick(folders().filter((f) => f !== "" && !f.startsWith("@wt/")));
      const to = inside(pick(folders()) ?? "", name());
      if (from !== undefined && !byPath.has(to) && !to.startsWith(`${from}/`) && to !== from) {
        const moved = Array.from(byPath.values()).filter(
          (n) => n.path === from || n.path.startsWith(`${from}/`),
        );
        dropTree(from);
        for (const n of moved)
          byPath.set(
            to + n.path.slice(from.length),
            treeNode(to + n.path.slice(from.length), n.kind),
          );
      }
    } else if (roll < 0.65) {
      // same path, other attributes (the backend removes and adds it)
      const path = pick(all);
      const old = path === undefined ? undefined : byPath.get(path);
      if (old !== undefined) byPath.set(old.path, { ...old, hidden: !old.hidden });
    } else if (roll < 0.72) {
      // a folder becomes a file of the same name (or the other way)
      const path = pick(all);
      const old = path === undefined ? undefined : byPath.get(path);
      if (old !== undefined) {
        dropTree(old.path);
        byPath.set(old.path, treeNode(old.path, old.kind.type === "dir" ? FILE : DIR));
      }
    } else if (roll < 0.8) {
      const root = `@wt/${pick(["one", "two", "three"]) ?? "one"}`;
      for (let i = 0; i < 3; i++) {
        const path = inside(root, name());
        if (!byPath.has(path)) byPath.set(path, treeNode(path));
      }
    } else if (roll < 0.85) {
      const root = pick(["@wt/one", "@wt/two", "@wt/three"]) ?? "@wt/one";
      for (const p of Array.from(byPath.keys())) if (p.startsWith(`${root}/`)) byPath.delete(p);
    } else {
      // several files into one folder at once
      const folder = pick(folders()) ?? "";
      for (let i = 0; i < 12; i++) {
        const path = inside(folder, `${name()} ${Math.floor(next() * 50)}`);
        if (!byPath.has(path)) byPath.set(path, treeNode(path));
      }
    }
  }
  return backendOrder(Array.from(byPath.values()));
}

describe("the kept name order", () => {
  it("equals a full sort after every random delta, and sorts in full only now and then", () => {
    for (const locale of ["en", "sv", "tr"]) {
      const next = rng(locale.length * 97);
      let list = backendOrder(bigList(300, { perFolder: 12, worktrees: ["one"], seed: 5 }));
      const order = new NameOrder();
      expect(paths(order.of(list, locale))).toEqual(paths(reference(list, locale)));
      let sorts = 0;
      const rounds = 300;
      for (let round = 0; round < rounds; round++) {
        const after = change(list, next);
        const body = diffLists(list, after);
        const d = delta(body);
        const applied = applyDelta(list, d);
        sorts += fullSorts(() => order.step(list, d, applied, locale));
        expect(order.of(applied, locale)).toEqual(reference(applied, locale));
        expect(order.paths(applied, locale)).toEqual(new Set(paths(applied)));
        list = applied;
      }
      // folders turned into files and back leave orphans now and then: those sort in full, the rest is kept
      expect(sorts).toBeLessThan(rounds / 4);
    }
  });

  it("an empty delta keeps the very same list", () => {
    const list = bigList(50);
    const order = new NameOrder();
    const before = order.of(list, "en");
    const applied = applyDelta(list, delta({}));
    expect(fullSorts(() => order.step(list, delta({}), applied, "en"))).toBe(0);
    expect(order.of(applied, "en")).toBe(before);
  });

  it("follows the full sort for deltas it cannot keep up with", () => {
    const list = backendOrder([
      treeNode("a", DIR),
      treeNode("a/x.md"),
      treeNode("b", DIR),
      treeNode("b/y.md"),
      treeNode("c.md"),
    ]);
    const cases: TreeDelta[] = [
      // a folder leaves while what it holds stays: its entries become a root of their own
      delta({ removed: ["a"] }),
      // placed after a node the same delta removed: `applyDelta` drops it
      delta({ removed: ["c.md"], added: [{ node: treeNode("d.md"), after: "c.md" }] }),
      // two nodes after one predecessor: only the last is placed
      delta({
        added: [
          { node: treeNode("d.md"), after: "c.md" },
          { node: treeNode("e.md"), after: "c.md" },
        ],
      }),
      // a path the list already has
      delta({ added: [{ node: treeNode("c.md"), after: "b/y.md" }] }),
      // after a path nobody has
      delta({ added: [{ node: treeNode("d.md"), after: "nowhere" }] }),
      // a new root ahead of the project's entries
      delta({ added: [{ node: treeNode("@wt/w/q.md"), after: null }] }),
    ];
    for (const d of cases) {
      const order = new NameOrder();
      order.of(list, "en");
      const applied = applyDelta(list, d);
      order.step(list, d, applied, "en");
      expect(paths(order.of(applied, "en"))).toEqual(paths(reference(applied, "en")));
      expect(order.paths(applied, "en")).toEqual(new Set(paths(applied)));
    }
  });

  it("a list it was not built from is sorted in full", () => {
    const order = new NameOrder();
    const one = bigList(40, { seed: 2 });
    const two = bigList(40, { seed: 3 });
    order.of(one, "en");
    const d = delta(diffLists(one, two));
    order.step(two, d, applyDelta(two, d), "en");
    const applied = applyDelta(two, d);
    expect(paths(order.of(applied, "en"))).toEqual(paths(reference(applied, "en")));
  });

  it("inNameOrder is the same full sort", () => {
    const list = bigList(2000, { worktrees: ["one", "two"], seed: 9 });
    for (const locale of ["en", "de", "ru"]) {
      expect(paths(inNameOrder(list, locale))).toEqual(paths(reference(list, locale)));
    }
  });

  it("one file added to a list of 100k entries is placed without sorting the list again", () => {
    const list = bigList(100_000, { worktrees: ["feature"] });
    const order = new NameOrder();
    order.of(list, "en");
    const d = delta({ added: [{ node: treeNode("new note 7.md"), after: list[0]?.path ?? null }] });
    const applied = applyDelta(list, d);
    let spent = 0;
    const sorts = fullSorts(() => {
      const started = performance.now();
      order.step(list, d, applied, "en");
      order.of(applied, "en");
      spent = performance.now() - started;
    });
    // the count is the guard: a full sort of this list (~70 ms) is what one change used to cost
    expect(sorts).toBe(0);
    // measured ~3-7 ms; the loose bound only catches work per change that grows faster than the list
    expect(spent).toBeLessThan(100);
    expect(paths(order.of(applied, "en"))).toEqual(paths(reference(applied, "en")));
  });
});
