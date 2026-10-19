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

const file: NodeKind = { type: "file", category: "markdown" };
const dir: NodeKind = { type: "dir", unlisted: null };

function node(path: string, kind: NodeKind = file): TreeNode {
  return { path, name: nameOf(path), kind, hidden: false, ignored: false, forced: false };
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

  it("handles a big list in one pass", () => {
    const big = Array.from({ length: 50_000 }, (_, n) => node(`f${n}.md`));
    const started = performance.now();
    expect(rowsOf([main], big, new Set(), new Set())).toHaveLength(50_000);
    expect(performance.now() - started).toBeLessThan(500);
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
