import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the Explorer lists for a tree it was given, and
// about when the tree is asked for again.
const fake = vi.hoisted(() => ({ listTree: vi.fn() }));
vi.mock("../api", () => ({
  api: {
    ...fake,
    saveSession: vi.fn(async () => {}),
    on: async () => () => {},
  },
}));
vi.mock("../platform", () => ({
  setWindowTitle: vi.fn(async () => {}),
  askUnsaved: vi.fn(async () => "keep"),
  askYesNo: vi.fn(async () => true),
  copyText: vi.fn(async () => true),
  guardWindowClose: vi.fn(async () => () => {}),
  watchFileDrop: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));

import { DEFAULT_TREE_FILTER } from "../treeFilter";
import type { TreeFilter, TreeNode } from "../types";
import { filter } from "./filter.svelte";
import { session } from "./session.svelte";
import { vault } from "./vault.svelte";

const file = (path: string): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "file",
});
const dir = (path: string, children: TreeNode[], extra: Partial<TreeNode> = {}): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "dir",
  children,
  ...extra,
});
const root = (children: TreeNode[]): TreeNode => ({
  name: "proj",
  path: "",
  kind: "dir",
  children,
});

// What the walker returns for a folder that holds a README, a CLAUDE.md, a script and a .gitignore, next to
// three repositories that the folder's .gitignore leaves out.
const TREE = root([
  dir(".github", [file(".github/ci.yml"), file(".github/guide.md")]),
  dir("Design", [file("Design/notes.md")]),
  file(".gitignore"),
  file("CLAUDE.md"),
  file("README.md"),
  file("start-team.bat"),
]);

const everything: TreeFilter = { ...DEFAULT_TREE_FILTER, show: "everything", categories: [] };
const names = (): string[] => filter.roots.main.map((n) => n.name);

describe("the Explorer filter store", () => {
  beforeEach(() => {
    fake.listTree.mockReset();
    fake.listTree.mockResolvedValue(TREE);
    vault.reset();
    session.treeFilter = null;
    filter.reset();
    vault.tree = TREE;
  });

  it("lists the documents, and the folders that hold them, by default", () => {
    expect(names()).toEqual(["Design", "CLAUDE.md", "README.md"]);
  });

  it("lists non-document files such as a script when the filter is Everything", () => {
    session.treeFilter = everything;
    expect(names()).toEqual(["Design", "CLAUDE.md", "README.md", "start-team.bat"]);
  });

  it("lists hidden files and folders too once they are asked for", () => {
    session.treeFilter = { ...everything, showHidden: true };
    expect(names()).toEqual([
      ".github",
      "Design",
      ".gitignore",
      "CLAUDE.md",
      "README.md",
      "start-team.bat",
    ]);
    session.treeFilter = { ...DEFAULT_TREE_FILTER, showHidden: true };
    expect(names()).toEqual([".github", "Design", "CLAUDE.md", "README.md"]);
  });

  it("counts the hidden files as hidden in the footer", () => {
    expect(filter.total).toBe(7);
    expect(filter.shownByFilter).toBe(3);
    expect(filter.hiddenByFilter).toBe(4);
    session.treeFilter = { ...everything, showHidden: true };
    expect(filter.hiddenByFilter).toBe(0);
  });

  it("asks for the tree again only when ignored or hidden files are switched on or off", () => {
    filter.set({ ...DEFAULT_TREE_FILTER, showIgnored: true });
    expect(fake.listTree).toHaveBeenLastCalledWith(true, false);
    expect(fake.listTree).toHaveBeenCalledTimes(1);

    filter.set({ ...DEFAULT_TREE_FILTER, showIgnored: true, showHidden: true });
    expect(fake.listTree).toHaveBeenLastCalledWith(true, true);
    expect(fake.listTree).toHaveBeenCalledTimes(2);

    // another change of the filter filters the tree that is there
    filter.set({ ...DEFAULT_TREE_FILTER, showIgnored: true, showHidden: true, show: "everything" });
    filter.set({ ...DEFAULT_TREE_FILTER, showIgnored: true, showHidden: true, patterns: "x" });
    expect(fake.listTree).toHaveBeenCalledTimes(2);

    filter.set(DEFAULT_TREE_FILTER);
    expect(fake.listTree).toHaveBeenLastCalledWith(false, false);
    expect(fake.listTree).toHaveBeenCalledTimes(3);
  });

  it("keeps an ignored folder that was too large to load, though it shows no file", () => {
    vault.tree = root([dir("empty", []), dir("target", [], { large: 2000 }), file("README.md")]);
    expect(names()).toEqual(["target", "README.md"]);
    expect(filter.roots.main[0].large).toBe(2000);
  });
});
