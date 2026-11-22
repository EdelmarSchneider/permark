import { describe, expect, it } from "vitest";
import type { NodeKind, RootNode, TreeDelta, TreeNode } from "../../api";
import {
  DEFAULT_FILTER,
  categoriesOf,
  filterToJson,
  normalizeFilter,
  sameFilter,
  withShow,
} from "./filter";
import {
  ancestorsOf,
  applyDelta,
  belowRoot,
  canDrop,
  depthOf,
  diffLists,
  dropBlock,
  edgeSpeed,
  inNameOrder,
  movesInto,
  nameOf,
  parentOf,
  rangeBetween,
  remapSet,
  renamedPath,
  rootOf,
  rowsOf,
  stemLength,
  topmost,
} from "./list";
import { rng, treeNode } from "../testing/trees";

const dir: NodeKind = { type: "dir", unlisted: null };
const node = treeNode;

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

const main: RootNode = { name: "p", kind: "main", path: "" };

// ---- spec ---------------------------------------------------------------------------------------------------------------

describe("paths", () => {
  it("name, folder, root and depth of project paths", () => {
    expect(nameOf("a/b/c.md")).toBe("c.md");
    expect(nameOf("c.md")).toBe("c.md");
    expect(parentOf("a/b/c.md")).toBe("a/b");
    expect(parentOf("c.md")).toBe("");
    expect(rootOf("a/b")).toBe("");
    expect(rootOf("@wt/x/a/b")).toBe("@wt/x");
    expect(rootOf("@wt/x")).toBe("@wt/x");
    expect(belowRoot("@wt/x/a/b")).toBe("a/b");
    expect(belowRoot("a/b")).toBe("a/b");
    expect(depthOf("a")).toBe(0);
    expect(depthOf("a/b/c.md")).toBe(2);
    expect(depthOf("@wt/x/a")).toBe(0);
    expect(depthOf("@wt/x/a/b")).toBe(1);
  });

  it("ancestors are outermost first, worktree root included", () => {
    expect(ancestorsOf("a/b/c.md")).toEqual(["a", "a/b"]);
    expect(ancestorsOf("c.md")).toEqual([]);
    expect(ancestorsOf("@wt/x/a/b.md")).toEqual(["@wt/x", "@wt/x/a"]);
  });

  it("the stem of a name is what a rename selects", () => {
    expect(stemLength("notes.md")).toBe(5);
    expect(stemLength("archive.tar.gz")).toBe(11);
    expect(stemLength(".env")).toBe(4);
    expect(stemLength("Makefile")).toBe(8);
    expect(stemLength("日本語.md")).toBe(3);
  });
});

describe("applyDelta", () => {
  it("drops removed nodes and inserts added ones after their predecessor", () => {
    const list = [node("a", dir), node("a/x.md"), node("b.md")];
    const next = applyDelta(
      list,
      delta({
        removed: ["a/x.md"],
        added: [
          { node: node("a/y.md"), after: "a" },
          { node: node("a/z.md"), after: "a/y.md" },
          { node: node("c.md"), after: "b.md" },
          { node: node("0.md"), after: null },
        ],
      }),
    );
    expect(next.map((n) => n.path)).toEqual(["0.md", "a", "a/y.md", "a/z.md", "b.md", "c.md"]);
  });

  it("a changed node is removed and added under the same path", () => {
    const list = [node("a", dir), node("b.md")];
    const folder: TreeNode = {
      ...node("a", dir),
      kind: { type: "dir", unlisted: { type: "ignored" } },
    };
    const next = applyDelta(
      list,
      delta({ removed: ["a"], added: [{ node: folder, after: null }] }),
    );
    expect(next).toEqual([folder, node("b.md")]);
  });

  it("an empty delta changes nothing", () => {
    const list = [node("a.md")];
    expect(applyDelta(list, delta({}))).toEqual(list);
  });

  it("is the inverse of diffLists over random lists", () => {
    let seed = 7;
    const next = (bound: number): number => {
      seed = (seed * 1_103_515_245 + 12_345) & 0x7f_ff_ff_ff;
      return seed % bound;
    };
    const pool = Array.from({ length: 40 }, (_, n) =>
      n % 5 === 0 ? node(`d${n}`, dir) : node(`f${n}.md`),
    );
    const pick = (): TreeNode[] => pool.filter(() => next(2) === 0);
    for (let round = 0; round < 200; round++) {
      const before = pick();
      const after = pick();
      const { removed, added } = diffLists(before, after);
      expect(applyDelta(before, delta({ removed, added }))).toEqual(after);
    }
  });
});

describe("rowsOf", () => {
  const nodes = [
    node("docs", dir),
    node("docs/img", dir),
    node("docs/img/p.png", { type: "file", category: "image" }),
    node("docs/a.md"),
    node("b.md"),
  ];

  it("shows a node when every folder above it is open", () => {
    const rows = (open: string[]) =>
      rowsOf([main], nodes, new Set(open), new Set()).map((r) =>
        r.type === "node" ? r.node.path : r.root.path,
      );
    expect(rows([])).toEqual(["docs", "b.md"]);
    expect(rows(["docs"])).toEqual(["docs", "docs/img", "docs/a.md", "b.md"]);
    expect(rows(["docs", "docs/img"])).toEqual([
      "docs",
      "docs/img",
      "docs/img/p.png",
      "docs/a.md",
      "b.md",
    ]);
    // an inner folder open under a closed one stays hidden
    expect(rows(["docs/img"])).toEqual(["docs", "b.md"]);
  });

  it("carries depth and open state", () => {
    const rows = rowsOf([main], nodes, new Set(["docs", "docs/img"]), new Set());
    const depths = rows.map((r) => (r.type === "node" ? [r.node.path, r.depth, r.expanded] : null));
    expect(depths).toEqual([
      ["docs", 0, true],
      ["docs/img", 1, true],
      ["docs/img/p.png", 2, false],
      ["docs/a.md", 1, false],
      ["b.md", 0, false],
    ]);
  });

  it("gives a worktree a header row that can be collapsed", () => {
    const wt: RootNode = { name: "feature", kind: "worktree", path: "@wt/feature" };
    const all = [...nodes.slice(4), node("@wt/feature/x", dir), node("@wt/feature/x/y.md")];
    const open = new Set(["@wt/feature/x"]);
    const types = (collapsed: string[]) =>
      rowsOf([main, wt], all, open, new Set(collapsed)).map((r) =>
        r.type === "root" ? `root:${r.root.name}` : r.node.path,
      );
    expect(types([])).toEqual(["b.md", "root:feature", "@wt/feature/x", "@wt/feature/x/y.md"]);
    expect(types(["@wt/feature"])).toEqual(["b.md", "root:feature"]);
  });

  it("a closed folder deep inside open ones hides only its own contents", () => {
    const deep = [
      node("a", dir),
      node("a/b", dir),
      node("a/b/c", dir),
      node("a/b/c/x.md"),
      node("a/b/y.md"),
      node("a/z.md"),
      node("q.md"),
    ];
    const shown = (open: string[]) =>
      rowsOf([main], deep, new Set(open), new Set()).map((r) =>
        r.type === "node" ? [r.node.path, r.depth] : [r.root.path, -1],
      );
    expect(shown(["a", "a/b/c"])).toEqual([
      ["a", 0],
      ["a/b", 1],
      ["a/z.md", 1],
      ["q.md", 0],
    ]);
    expect(shown(["a", "a/b", "a/b/c"])).toEqual([
      ["a", 0],
      ["a/b", 1],
      ["a/b/c", 2],
      ["a/b/c/x.md", 3],
      ["a/b/y.md", 2],
      ["a/z.md", 1],
      ["q.md", 0],
    ]);
  });

  it("a node whose folder is not listed keeps its depth by path; a new root starts with nothing closed", () => {
    const wt: RootNode = { name: "feature", kind: "worktree", path: "@wt/feature" };
    const odd = [
      node("a", dir),
      node("a/b/c.md"),
      node("z", dir),
      node("z/hidden.md"),
      node("@wt/feature/d", dir),
      node("@wt/feature/d/e.md"),
      node("@wt/feature/f/g.md"),
    ];
    const rows = rowsOf([main, wt], odd, new Set(["a", "@wt/feature/d"]), new Set());
    expect(
      rows.map((r) => (r.type === "node" ? [r.node.path, r.depth] : [r.root.path, -1])),
    ).toEqual([
      ["a", 0],
      ["a/b/c.md", 2],
      ["z", 0],
      ["@wt/feature", -1],
      ["@wt/feature/d", 0],
      ["@wt/feature/d/e.md", 1],
      ["@wt/feature/f/g.md", 1],
    ]);
  });

  it("handles a big list in one pass", () => {
    const big = Array.from({ length: 50_000 }, (_, n) => node(`f${n}.md`));
    const started = performance.now();
    expect(rowsOf([main], big, new Set(), new Set())).toHaveLength(50_000);
    expect(performance.now() - started).toBeLessThan(500);
  });
});

describe("name order", () => {
  const paths = (list: readonly TreeNode[]): string[] => list.map((n) => n.path);
  const sorted = (locale: string, names: string[]): string[] =>
    paths(
      inNameOrder(
        names.map((name) => node(name)),
        locale,
      ),
    );

  it("folders first, then names as the language sorts them, numbers by their value", () => {
    const list = [
      node("b.md"),
      node("z", dir),
      node("file 10.md"),
      node("a", dir),
      node("file 2.md"),
      node("File 1.md"),
    ];
    expect(paths(inNameOrder(list, "en"))).toEqual([
      "a",
      "z",
      "b.md",
      "File 1.md",
      "file 2.md",
      "file 10.md",
    ]);
  });

  it("Russian: Ё beside Е, Cyrillic names before Latin ones", () => {
    expect(sorted("ru", ["Ёж.md", "Дом.md", "Жук.md", "Яблоко.md", "Ель.md", "apple.md"])).toEqual([
      "Дом.md",
      "Ёж.md",
      "Ель.md",
      "Жук.md",
      "Яблоко.md",
      "apple.md",
    ]);
  });

  it("Vietnamese: Ă, Â, Đ, Ô, Ơ beside their base letters, not after Z", () => {
    expect(
      sorted("vi", ["Zebra.md", "Ơn.md", "Đà.md", "Ăn.md", "Ôm.md", "An.md", "Dê.md", "On.md"]),
    ).toEqual(["An.md", "Ăn.md", "Dê.md", "Đà.md", "On.md", "Ôm.md", "Ơn.md", "Zebra.md"]);
  });

  it("Turkish: Ç, Ö, Ş beside C, O, S", () => {
    expect(sorted("tr", ["Şeker.md", "Tas.md", "Öykü.md", "Pul.md", "Çay.md", "Dal.md"])).toEqual([
      "Çay.md",
      "Dal.md",
      "Öykü.md",
      "Pul.md",
      "Şeker.md",
      "Tas.md",
    ]);
    expect(sorted("tr", ["Su.md", "Cam.md", "Ozan.md", "Şeker.md", "Çay.md", "Öykü.md"])).toEqual([
      "Cam.md",
      "Çay.md",
      "Ozan.md",
      "Öykü.md",
      "Su.md",
      "Şeker.md",
    ]);
  });

  it("names the language sees as equal keep one fixed order", () => {
    expect(sorted("en", ["a.md", "A.md"])).toEqual(["A.md", "a.md"]);
    expect(sorted("en", ["A.md", "a.md"])).toEqual(["A.md", "a.md"]);
  });

  it("orders each folder's entries under it; worktrees keep their place after the project", () => {
    const list = [
      node("b", dir),
      node("b/z.md"),
      node("b/a.md"),
      node("a.md"),
      node("@wt/x/q.md"),
      node("@wt/x/p.md"),
    ];
    expect(paths(inNameOrder(list, "en"))).toEqual([
      "b",
      "b/a.md",
      "b/z.md",
      "a.md",
      "@wt/x/p.md",
      "@wt/x/q.md",
    ]);
  });
});

describe("selection helpers", () => {
  const rows = rowsOf(
    [main],
    [node("a.md"), node("b.md"), node("c.md"), node("d.md")],
    new Set(),
    new Set(),
  );

  it("a range runs between two rows in either order", () => {
    expect(rangeBetween(rows, "b.md", "d.md")).toEqual(["b.md", "c.md", "d.md"]);
    expect(rangeBetween(rows, "d.md", "b.md")).toEqual(["b.md", "c.md", "d.md"]);
    expect(rangeBetween(rows, "b.md", "b.md")).toEqual(["b.md"]);
    expect(rangeBetween(rows, "gone.md", "c.md")).toEqual(["c.md"]);
  });

  it("topmost drops what a selected folder already takes along", () => {
    expect(topmost(["a", "a/b.md", "a/c/d.md", "z.md", "a"]).sort()).toEqual(["a", "z.md"]);
    expect(topmost(["ab", "a"]).sort()).toEqual(["a", "ab"]);
    expect(topmost([])).toEqual([]);
  });

  it("topmost keeps what the pairwise check kept, in the same order, for random nested and worktree paths", () => {
    // the first version: each path against every other one
    const pairwise = (list: Iterable<string>): string[] => {
      const all = Array.from(new Set(list));
      return all.filter((p) => !all.some((other) => other !== p && p.startsWith(`${other}/`)));
    };
    const next = rng(11);
    const parts = ["a", "b", "ab", "a b", "@wt", "x", "é", ""];
    const part = (): string => parts[Math.floor(next() * parts.length)] ?? "a";
    for (let round = 0; round < 500; round++) {
      const picked = Array.from({ length: 1 + Math.floor(next() * 12) }, () => {
        const depth = 1 + Math.floor(next() * 4);
        const path = Array.from({ length: depth }, part).join("/");
        return next() < 0.3 ? `@wt/${path}` : path;
      });
      expect(topmost(picked)).toEqual(pairwise(picked));
    }
  });

  it("topmost of 50k selected sibling rows takes well under a second", () => {
    const rows = Array.from({ length: 50_000 }, (_, n) => `docs/f${n}.md`);
    const started = performance.now();
    expect(topmost([...rows, "docs/f7.md/inner"])).toHaveLength(50_000);
    expect(performance.now() - started).toBeLessThan(1000);
  });
});

describe("renames", () => {
  it("paths follow a renamed folder", () => {
    const renames = [{ from: "old", to: "new/old2" }];
    expect(renamedPath("old", renames)).toBe("new/old2");
    expect(renamedPath("old/x.md", renames)).toBe("new/old2/x.md");
    expect(renamedPath("older/x.md", renames)).toBe("older/x.md");
    expect(remapSet(new Set(["old", "old/a", "z"]), renames)).toEqual(
      new Set(["new/old2", "new/old2/a", "z"]),
    );
    expect(remapSet(new Set(["a"]), [])).toEqual(new Set(["a"]));
  });
});

describe("drag and drop rules", () => {
  it("a folder takes things from elsewhere, never from where they are, never into themselves", () => {
    expect(canDrop(["a.md"], "docs")).toBe(true);
    expect(canDrop(["docs/a.md"], "")).toBe(true);
    expect(canDrop(["docs/a.md"], "docs")).toBe(false);
    expect(canDrop(["docs"], "docs")).toBe(false);
    expect(canDrop(["docs"], "docs/sub")).toBe(false);
    expect(canDrop(["docs", "a.md"], "docs/sub")).toBe(false);
    expect(canDrop(["docs"], "docsx")).toBe(true);
    expect(canDrop([], "docs")).toBe(false);
    // between roots is refused
    expect(canDrop(["a.md"], "@wt/x")).toBe(false);
    expect(canDrop(["@wt/x/a.md"], "@wt/x/dir")).toBe(true);
    expect(canDrop(["@wt/x/a.md"], "")).toBe(false);
  });

  it("moves keep the names", () => {
    expect(movesInto(["a.md", "docs/b.md"], "x")).toEqual([
      { from: "a.md", to: "x/a.md" },
      { from: "docs/b.md", to: "x/b.md" },
    ]);
    expect(movesInto(["docs/b.md"], "")).toEqual([{ from: "docs/b.md", to: "b.md" }]);
  });
});

describe("filter helpers", () => {
  it("a preset's categories, and custom keeps the previous preset's when switched to", () => {
    expect(categoriesOf("documents")).toEqual(["markdown", "text"]);
    expect(categoriesOf("custom")).toEqual([]);
    const custom = withShow(DEFAULT_FILTER, "custom");
    expect(custom.categories).toEqual(DEFAULT_FILTER.categories);
    expect(withShow(custom, "custom")).toBe(custom);
    expect(withShow(custom, "documents").show).toBe("documents");
  });

  it("selecting Everything turns ignored and hidden files on, including a second time", () => {
    const on = withShow({ ...DEFAULT_FILTER, hideEmptyDirs: false }, "everything");
    expect(on.show).toBe("everything");
    expect(on.showIgnored).toBe(true);
    expect(on.showHidden).toBe(true);
    expect(on.hideEmptyDirs).toBe(false);

    const again = withShow({ ...on, showIgnored: false }, "everything");
    expect(again.show).toBe("everything");
    expect(again.showIgnored).toBe(true);
    expect(again.showHidden).toBe(true);
    expect(again.hideEmptyDirs).toBe(false);
  });

  it("documents and docs-and-assets leave the show flags and empty folders as they were", () => {
    expect(DEFAULT_FILTER.show).toBe("docs-and-assets");
    expect(DEFAULT_FILTER.showIgnored).toBe(false);
    expect(DEFAULT_FILTER.showHidden).toBe(false);

    const docs = withShow(DEFAULT_FILTER, "documents");
    expect(docs.show).toBe("documents");
    expect(docs.showIgnored).toBe(false);
    expect(docs.showHidden).toBe(false);
    expect(docs.hideEmptyDirs).toBe(true);

    const kept = withShow(
      {
        ...DEFAULT_FILTER,
        show: "everything",
        showIgnored: true,
        showHidden: true,
        hideEmptyDirs: false,
      },
      "documents",
    );
    expect(kept.show).toBe("documents");
    expect(kept.showIgnored).toBe(true);
    expect(kept.showHidden).toBe(true);
    expect(kept.hideEmptyDirs).toBe(false);

    const assets = withShow(
      { ...DEFAULT_FILTER, showIgnored: true, showHidden: false, hideEmptyDirs: false },
      "docs-and-assets",
    );
    expect(assets.show).toBe("docs-and-assets");
    expect(assets.showIgnored).toBe(true);
    expect(assets.showHidden).toBe(false);
    expect(assets.hideEmptyDirs).toBe(false);
  });

  it("a remembered filter is checked field by field", () => {
    expect(normalizeFilter(filterToJson(DEFAULT_FILTER))).toEqual(DEFAULT_FILTER);
    expect(normalizeFilter(null)).toBeNull();
    expect(normalizeFilter("x")).toBeNull();
    expect(normalizeFilter({ show: "nope" })).toBeNull();
    expect(normalizeFilter(7)).toBeNull();
    const odd = normalizeFilter({
      show: "custom",
      categories: ["markdown", "markdown", "bogus", 3],
      patterns: 5,
      showHidden: "yes",
      hideEmptyDirs: false,
    });
    expect(odd).toEqual({
      show: "custom",
      categories: ["markdown"],
      patterns: "",
      assetsNearDocsOnly: true,
      hideEmptyDirs: false,
      showHidden: false,
      showIgnored: false,
    });
  });

  it("same filters compare equal whatever the order of the categories", () => {
    const a = {
      ...DEFAULT_FILTER,
      show: "custom" as const,
      categories: ["markdown" as const, "text" as const],
    };
    const b = { ...a, categories: ["text" as const, "markdown" as const] };
    expect(sameFilter(a, b)).toBe(true);
    expect(sameFilter(a, { ...b, patterns: "+x" })).toBe(false);
  });
});

// ---- adversarial ----------------------------------------------------------------------------------------------------------

describe("hostile input", () => {
  it("odd names and paths never break the helpers", () => {
    for (const path of [
      "",
      "/",
      "//",
      "a//b",
      "@wt",
      "@wt/",
      "@wt//x",
      " ",
      "日本/語",
      "a/b/",
      ".",
      "..",
    ]) {
      expect(() => {
        nameOf(path);
        parentOf(path);
        rootOf(path);
        belowRoot(path);
        depthOf(path);
        ancestorsOf(path);
        renamedPath(path, [{ from: "a", to: "b" }]);
        canDrop([path], "x");
      }, JSON.stringify(path)).not.toThrow();
    }
  });

  it("a delta naming unknown paths or a missing predecessor is applied without failing", () => {
    const list = [node("a.md")];
    const next = applyDelta(
      list,
      delta({ removed: ["ghost"], added: [{ node: node("late.md"), after: "ghost" }] }),
    );
    expect(next.map((n) => n.path)).toEqual(["a.md"]);
  });
});

// ---- drag from outside -------------------------------------------------------------------------------------------------

describe("dropBlock", () => {
  const wt: RootNode = { name: "feature", kind: "worktree", path: "@wt/feature" };
  const nodes = [
    node("docs", dir),
    node("docs/deep", dir),
    node("docs/deep/x.md"),
    node("docs/a.md"),
    node("README.md"),
    node("@wt/feature/b.md"),
  ];
  const rows = rowsOf([wt], nodes, new Set(["docs", "docs/deep"]), new Set());
  const keys = (range: { first: number; last: number } | null): string[] =>
    range === null
      ? []
      : rows
          .slice(range.first, range.last + 1)
          .map((r) => (r.type === "root" ? r.root.path : r.node.path));

  it("a folder: its visible contents, deeper rows only", () => {
    expect(keys(dropBlock(rows, "docs"))).toEqual(["docs/deep", "docs/deep/x.md", "docs/a.md"]);
    expect(keys(dropBlock(rows, "docs/deep"))).toEqual(["docs/deep/x.md"]);
  });

  it("a worktree root: its rows", () => {
    expect(keys(dropBlock(rows, "@wt/feature"))).toEqual(["@wt/feature/b.md"]);
  });

  it("nothing to block: a closed folder, the project folder, no target, a path not shown", () => {
    const closed = rowsOf([], nodes, new Set(), new Set());
    expect(dropBlock(closed, "docs")).toBeNull();
    expect(dropBlock(rows, "")).toBeNull();
    expect(dropBlock(rows, null)).toBeNull();
    expect(dropBlock(rows, "gone")).toBeNull();
    expect(dropBlock([], "docs")).toBeNull();
  });
});

describe("edgeSpeed", () => {
  it("scrolls near the edges, faster closer to them, never in the middle or outside", () => {
    expect(edgeSpeed(100, 0, 400)).toBe(0);
    expect(edgeSpeed(0, 0, 400)).toBe(-14);
    expect(edgeSpeed(27, 0, 400)).toBe(-2);
    expect(edgeSpeed(14, 0, 400)).toBe(-8);
    expect(edgeSpeed(400, 0, 400)).toBe(14);
    expect(edgeSpeed(373, 0, 400)).toBe(2);
    expect(edgeSpeed(28, 0, 400)).toBe(0);
    expect(edgeSpeed(-5, 0, 400)).toBe(0);
    expect(edgeSpeed(401, 0, 400)).toBe(0);
  });

  it("a list smaller than both zones splits it in halves; an empty one never scrolls", () => {
    expect(edgeSpeed(5, 0, 20)).toBeLessThan(0);
    expect(edgeSpeed(15, 0, 20)).toBeGreaterThan(0);
    expect(edgeSpeed(0, 10, 10)).toBe(0);
    expect(edgeSpeed(Number.NaN, 0, 400)).toBe(0);
  });
});
