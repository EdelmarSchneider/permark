import { describe, expect, it } from "vitest";
import { batchTouches, folderCounts, nextUnread, onlyUnread, unreadWalk } from "./unread";
import type { FsBatch, TreeNode } from "./types";

const file = (path: string): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "file",
});
const dir = (path: string, children: TreeNode[], kind: "dir" | "worktree" = "dir"): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind,
  children,
});

const TREE = [
  dir("docs", [
    dir("docs/specs", [file("docs/specs/a.md"), file("docs/specs/b.md")]),
    file("docs/intro.md"),
  ]),
  dir("empty", []),
  file("README.md"),
  dir(
    "@wt/feature-ui",
    [dir("@wt/feature-ui/docs", [file("@wt/feature-ui/docs/ui.md")])],
    "worktree",
  ),
];

describe("folderCounts", () => {
  it("counts an unread document for every folder above it", () => {
    const counts = folderCounts([
      "docs/specs/a.md",
      "docs/specs/b.md",
      "docs/intro.md",
      "README.md",
    ]);
    expect(counts.get("docs")).toBe(3);
    expect(counts.get("docs/specs")).toBe(2);
    expect(counts.has("README.md")).toBe(false);
    expect(counts.has("empty")).toBe(false);
  });

  it("counts the files of a worktree for the worktree and its folders", () => {
    const counts = folderCounts(["@wt/feature-ui/docs/ui.md"]);
    expect(counts.get("@wt/feature-ui")).toBe(1);
    expect(counts.get("@wt/feature-ui/docs")).toBe(1);
    expect(counts.size).toBe(2);
  });

  it("is empty when everything is read", () => {
    expect(folderCounts([]).size).toBe(0);
  });
});

describe("onlyUnread", () => {
  it("keeps the unread documents and the folders on the way to them", () => {
    const shown = onlyUnread(TREE, new Set(["docs/specs/b.md", "@wt/feature-ui/docs/ui.md"]), null);
    expect(shown.map((n) => n.path)).toEqual(["docs", "@wt/feature-ui"]);
    expect(shown[0].children?.map((n) => n.path)).toEqual(["docs/specs"]);
    expect(shown[0].children?.[0].children?.map((n) => n.path)).toEqual(["docs/specs/b.md"]);
    expect(shown[1].kind).toBe("worktree");
  });

  it("lists nothing when nothing is unread", () => {
    expect(onlyUnread(TREE, new Set(), null)).toEqual([]);
  });

  it("keeps the open document, read or not, so it does not vanish when it counts as read", () => {
    const shown = onlyUnread(TREE, new Set(["README.md"]), "docs/intro.md");
    expect(shown.map((n) => n.path)).toEqual(["docs", "README.md"]);
    expect(shown[0].children?.map((n) => n.path)).toEqual(["docs/intro.md"]);
  });

  it("does not touch the tree it was given", () => {
    const before = JSON.stringify(TREE);
    onlyUnread(TREE, new Set(["docs/intro.md"]), null);
    expect(JSON.stringify(TREE)).toBe(before);
  });
});

describe("the walk of next unread", () => {
  const tree = [
    "docs/intro.md",
    "docs/specs/a.md",
    "docs/specs/b.md",
    "README.md",
    "@wt/feature-ui/docs/ui.md",
  ];

  it("goes through the feed, newest first, and then the rest in tree order", () => {
    const unread = new Set(["README.md", "docs/intro.md", "docs/specs/b.md", "docs/specs/a.md"]);
    const feed = ["docs/specs/b.md", "notes.txt", "README.md", "docs/gone.md"];
    expect(unreadWalk({ unread, feed, tree })).toEqual([
      "docs/specs/b.md",
      "README.md",
      "docs/intro.md",
      "docs/specs/a.md",
    ]);
  });

  it("lists an unread document the tree does not know last", () => {
    const unread = new Set(["zzz.md", "README.md"]);
    expect(unreadWalk({ unread, feed: [], tree })).toEqual(["README.md", "zzz.md"]);
  });

  it("lists each document once", () => {
    const unread = new Set(["README.md"]);
    expect(unreadWalk({ unread, feed: ["README.md", "README.md"], tree })).toEqual(["README.md"]);
  });
});

describe("nextUnread", () => {
  const tree = ["a.md", "b.md", "c.md", "d.md"];

  it("opens the first one when the document on show is read", () => {
    const unread = new Set(["b.md", "c.md"]);
    expect(nextUnread({ unread, feed: [], tree, current: "a.md" })).toBe("b.md");
    expect(nextUnread({ unread, feed: [], tree, current: null })).toBe("b.md");
  });

  it("goes on to the one after the document on show, even while that one is still unread", () => {
    const unread = new Set(["a.md", "b.md", "c.md"]);
    expect(nextUnread({ unread, feed: [], tree, current: "a.md" })).toBe("b.md");
    expect(nextUnread({ unread, feed: [], tree, current: "b.md" })).toBe("c.md");
    expect(nextUnread({ unread, feed: [], tree, current: "c.md" })).toBe("a.md");
  });

  it("follows the feed first", () => {
    const unread = new Set(["a.md", "d.md"]);
    expect(nextUnread({ unread, feed: ["d.md", "a.md"], tree, current: null })).toBe("d.md");
    expect(nextUnread({ unread, feed: ["d.md", "a.md"], tree, current: "d.md" })).toBe("a.md");
  });

  it("finds nothing when everything is read, or when the only unread document is the one on show", () => {
    expect(nextUnread({ unread: new Set(), feed: [], tree, current: "a.md" })).toBeNull();
    expect(nextUnread({ unread: new Set(["a.md"]), feed: [], tree, current: "a.md" })).toBeNull();
  });
});

describe("batchTouches", () => {
  const batch = (extra: Partial<FsBatch>): FsBatch => ({
    changes: [],
    bulk: null,
    rescan: false,
    ...extra,
  });

  it("sees the file among the changes, on either side of a rename", () => {
    const change = { kind: "modified" as const, path: "a.md", atMs: 1 };
    expect(batchTouches(batch({ changes: [change] }), "a.md")).toBe(true);
    expect(batchTouches(batch({ changes: [change] }), "b.md")).toBe(false);
    const renamed = { kind: "renamed" as const, path: "new.md", from: "old.md", atMs: 1 };
    expect(batchTouches(batch({ changes: [renamed] }), "old.md")).toBe(true);
  });

  it("sees the file among the paths of a bulk batch", () => {
    const bulk = { count: 90, reason: "gitOperation" as const, paths: ["a.md"] };
    expect(batchTouches(batch({ bulk }), "a.md")).toBe(true);
    expect(batchTouches(batch({ bulk }), "b.md")).toBe(false);
  });

  it("takes a rescan for a change of anything", () => {
    expect(batchTouches(batch({ rescan: true }), "a.md")).toBe(true);
  });
});
