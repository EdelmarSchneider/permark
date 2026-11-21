// Timings of the Explorer list logic at sizes a big project reaches (`npx vitest bench --run <this file>`). Not part
// of the test run: the counted and bounded checks live in the tests.

import { test } from "vitest";
import type { TreeDelta, TreeNode } from "../../api";
import { bigList, treeNode } from "../testing/trees";
import { applyDelta, inNameOrder, rowsOf, topmost } from "./list";
import { NameOrder } from "./order";

const SIZES = [20_000, 100_000] as const;
const lists = new Map<number, TreeNode[]>(
  SIZES.map((n) => [n, bigList(n, { worktrees: ["feature"] })] as const),
);
const listOf = (n: number): TreeNode[] => lists.get(n) ?? [];
const roots = [
  { name: "p", kind: "main" as const, path: "" },
  { name: "feature", kind: "worktree" as const, path: "@wt/feature" },
];
const options = { time: 300, iterations: 3 };

/** One file created at the top of the project. */
function oneAdded(list: readonly TreeNode[]): TreeDelta {
  return {
    seq: 1,
    removed: [],
    added: [{ node: treeNode("new note.md"), after: list[0]?.path ?? null }],
    renamed: [],
    totalFiles: 0,
    shownFiles: 0,
    bulk: false,
    filter: null,
  };
}

test("inNameOrder", async ({ bench }) => {
  await bench.compare(
    ...SIZES.map((n) => bench(`${n}`, () => void inNameOrder(listOf(n), "en"))),
    options,
  );
});

test("rowsOf, every folder open", async ({ bench }) => {
  const runs = SIZES.map((n) => {
    const ordered = inNameOrder(listOf(n), "en");
    const open = new Set(ordered.filter((x) => x.kind.type === "dir").map((x) => x.path));
    return bench(`${n}`, () => void rowsOf(roots, ordered, open, new Set()));
  });
  await bench.compare(...runs, options);
});

test("applyDelta, one file added", async ({ bench }) => {
  const runs = SIZES.map((n) => {
    const list = listOf(n);
    const delta = oneAdded(list);
    return bench(`${n}`, () => void applyDelta(list, delta));
  });
  await bench.compare(...runs, options);
});

test("one file added: list + name order", async ({ bench }) => {
  const runs = SIZES.map((n) => {
    const list = listOf(n);
    const delta = oneAdded(list);
    return bench(`${n}`, () => void inNameOrder(applyDelta(list, delta), "en"));
  });
  await bench.compare(...runs, options);
});

test("one file added, then removed: list + kept name order (two deltas per run)", async ({
  bench,
}) => {
  const runs = SIZES.map((n) => {
    const add = oneAdded(listOf(n));
    const remove: TreeDelta = { ...add, added: [], removed: ["new note.md"] };
    const order = new NameOrder();
    let current = listOf(n);
    order.of(current, "en");
    return bench(`${n}`, () => {
      const after = applyDelta(current, add);
      order.step(current, add, after, "en");
      order.of(after, "en");
      const back = applyDelta(after, remove);
      order.step(after, remove, back, "en");
      order.of(back, "en");
      current = back;
    });
  });
  await bench.compare(...runs, options);
});

test("topmost, sibling rows selected", async ({ bench }) => {
  const runs = SIZES.map((n) => {
    const paths = Array.from({ length: n }, (_, i) => `docs/f${i}.md`);
    return bench(`${n}`, () => void topmost(paths));
  });
  await bench.compare(...runs, options);
});
