import { describe, expect, it } from "vitest";
import {
  badgeFor,
  childNames,
  collectFiles,
  collectFolders,
  findNode,
  flattenTree,
  isDirPath,
  navigate,
  rollups,
  splitRoots,
  typeahead,
} from "./tree";
import type { TreeNode } from "./types";

const file = (path: string): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "file",
});
const dir = (path: string, children: TreeNode[]): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "dir",
  children,
});

const root: TreeNode = dir("", [
  dir("docs", [dir("docs/specs", [file("docs/specs/live-feed.md")]), file("docs/index.md")]),
  file("README.md"),
  {
    name: "feature-ui",
    path: "@wt/feature-ui",
    kind: "worktree",
    children: [file("@wt/feature-ui/a.md")],
  },
]);

describe("splitRoots / collectFiles / findNode", () => {
  it("separates worktree roots from the main tree", () => {
    const { main, worktrees } = splitRoots(root);
    expect(main.map((n) => n.name)).toEqual(["docs", "README.md"]);
    expect(worktrees.map((n) => n.path)).toEqual(["@wt/feature-ui"]);
  });

  it("collects every folder, nested ones and worktree roots included, and no file", () => {
    expect(collectFolders(root.children ?? [])).toEqual(["docs", "docs/specs", "@wt/feature-ui"]);
    expect(collectFolders([file("README.md")])).toEqual([]);
    expect(collectFolders([])).toEqual([]);
  });

  it("collects every file including worktree files", () => {
    expect(collectFiles(root.children ?? [])).toEqual([
      "docs/specs/live-feed.md",
      "docs/index.md",
      "README.md",
      "@wt/feature-ui/a.md",
    ]);
  });

  it("finds nodes by path", () => {
    expect(findNode(root.children ?? [], "docs/specs")?.kind).toBe("dir");
    expect(findNode(root.children ?? [], "nope")).toBeNull();
  });
});

describe("flattenTree", () => {
  const { main } = splitRoots(root);

  it("shows only top-level rows when nothing is expanded", () => {
    expect(flattenTree(main, new Set()).map((r) => r.node.name)).toEqual(["docs", "README.md"]);
  });

  it("includes children of expanded folders with their depth", () => {
    const rows = flattenTree(main, new Set(["docs", "docs/specs"]));
    expect(rows.map((r) => [r.node.name, r.depth])).toEqual([
      ["docs", 0],
      ["specs", 1],
      ["live-feed.md", 2],
      ["index.md", 1],
      ["README.md", 0],
    ]);
    expect(rows[0].expanded).toBe(true);
    expect(rows[4].isDir).toBe(false);
  });
});

describe("rollups", () => {
  it("lets the worst state win in every ancestor folder", () => {
    const map = rollups({
      "docs/specs/a.md": { state: "untracked", staged: false },
      "docs/specs/b.md": { state: "modified", staged: false },
      "docs/c.md": { state: "deleted", staged: false },
      "top/d.md": { state: "added", staged: false },
    });
    expect(map.get("docs/specs")).toBe("modified");
    expect(map.get("docs")).toBe("deleted");
    expect(map.get("top")).toBe("added");
    expect(map.has("README.md")).toBe(false);
  });

  it("ranks conflicted above deleted and treats renamed as modified", () => {
    const map = rollups({
      "a/x.md": { state: "conflicted", staged: false },
      "a/y.md": { state: "deleted", staged: false },
      "b/z.md": { state: "renamed", staged: false, from: "b/old.md" },
    });
    expect(map.get("a")).toBe("conflicted");
    expect(map.get("b")).toBe("modified");
  });
});

describe("badgeFor", () => {
  it("maps git states to badge letters", () => {
    expect(badgeFor("modified")?.letter).toBe("M");
    expect(badgeFor("untracked")?.letter).toBe("U");
    expect(badgeFor("conflicted")?.letter).toBe("C");
    expect(badgeFor(undefined)).toBeNull();
  });
});

describe("navigate", () => {
  const rows = flattenTree(splitRoots(root).main, new Set(["docs"]));
  // docs(0) specs(1) index.md(2) README.md(3)

  it("moves with the arrow keys and stays inside the list", () => {
    expect(navigate(rows, 0, "ArrowDown")).toEqual({ focus: 1 });
    expect(navigate(rows, 3, "ArrowDown")).toEqual({ focus: 3 });
    expect(navigate(rows, 0, "ArrowUp")).toEqual({ focus: 0 });
    expect(navigate(rows, 1, "Home")).toEqual({ focus: 0 });
    expect(navigate(rows, 1, "End")).toEqual({ focus: 3 });
  });

  it("collapses an open folder on Left, then moves to the parent", () => {
    expect(navigate(rows, 0, "ArrowLeft")).toEqual({
      focus: 0,
      toggle: { path: "docs", expand: false },
    });
    expect(navigate(rows, 2, "ArrowLeft")).toEqual({ focus: 0 });
  });

  it("expands a closed folder on Right, then enters it", () => {
    expect(navigate(rows, 1, "ArrowRight")).toEqual({
      focus: 1,
      toggle: { path: "docs/specs", expand: true },
    });
    expect(navigate(rows, 0, "ArrowRight")).toEqual({ focus: 1 });
  });

  it("opens files and toggles folders on Enter and Space", () => {
    expect(navigate(rows, 2, "Enter")).toEqual({ focus: 2, open: "docs/index.md" });
    expect(navigate(rows, 0, " ")).toEqual({ focus: 0, toggle: { path: "docs", expand: false } });
  });

  it("ignores other keys and empty lists", () => {
    expect(navigate(rows, 0, "a")).toBeNull();
    expect(navigate([], 0, "ArrowDown")).toBeNull();
  });
});

describe("childNames / isDirPath", () => {
  it("lists the names inside a folder, files and folders alike", () => {
    expect(childNames(root, "docs")).toEqual(["specs", "index.md"]);
    expect(childNames(root, "docs/specs")).toEqual(["live-feed.md"]);
    expect(childNames(root, "@wt/feature-ui")).toEqual(["a.md"]);
  });

  it("lists the project root without its worktrees", () => {
    expect(childNames(root, "")).toEqual(["docs", "README.md"]);
  });

  it("is empty for files, unknown folders and no tree", () => {
    expect(childNames(root, "README.md")).toEqual([]);
    expect(childNames(root, "nope")).toEqual([]);
    expect(childNames(null, "")).toEqual([]);
  });

  it("tells folders from files", () => {
    expect(isDirPath(root, "docs")).toBe(true);
    expect(isDirPath(root, "@wt/feature-ui")).toBe(true);
    expect(isDirPath(root, "docs/index.md")).toBe(false);
    expect(isDirPath(root, "nope")).toBe(false);
    expect(isDirPath(null, "docs")).toBe(false);
  });
});

describe("typeahead", () => {
  const rows = flattenTree(splitRoots(root).main, new Set(["docs"]));
  it("jumps to the next row starting with the typed text", () => {
    expect(typeahead(rows, 0, "r")).toBe(3);
    expect(typeahead(rows, 0, "in")).toBe(2);
    expect(typeahead(rows, 0, "zz")).toBeNull();
  });
});
