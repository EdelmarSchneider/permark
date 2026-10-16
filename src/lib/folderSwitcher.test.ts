import { describe, expect, it } from "vitest";
import {
  folderKey,
  parentLabel,
  parentOf,
  sameFolder,
  switchable,
  switcherRows,
} from "./folderSwitcher";
import type { RecentVault } from "./types";

const recent = (root: string, name: string, lastOpenedMs = 1): RecentVault => ({
  root,
  name,
  lastOpenedMs,
});

const NONE: ReadonlySet<string> = new Set();

describe("folderKey and sameFolder", () => {
  it("ignores the separator style and a trailing separator", () => {
    expect(sameFolder("F:\\work\\docs", "F:/work/docs/")).toBe(true);
    expect(sameFolder("/home/ana/docs", "/home/ana/docs/")).toBe(true);
    expect(sameFolder("/home/ana/docs", "/home/ana/other")).toBe(false);
  });

  it("ignores the case of a Windows path, and of nothing else", () => {
    expect(sameFolder("F:\\Work\\Docs", "f:\\work\\docs")).toBe(true);
    expect(sameFolder("\\\\server\\Share\\Docs", "\\\\SERVER\\share\\docs")).toBe(true);
    // a path on a case-sensitive file system keeps its case
    expect(sameFolder("/home/ana/Docs", "/home/ana/docs")).toBe(false);
  });

  it("keeps the separator of a root", () => {
    expect(folderKey("C:\\")).toBe("c:/");
    expect(folderKey("/")).toBe("/");
  });

  it("is never the same as nothing", () => {
    expect(sameFolder("F:/work", null)).toBe(false);
    expect(sameFolder(undefined, null)).toBe(false);
  });
});

describe("parentOf", () => {
  it("is the folder that holds the path, in the separators the path has", () => {
    expect(parentOf("F:\\work\\big-repo")).toBe("F:\\work");
    expect(parentOf("F:/work/big-repo")).toBe("F:/work");
    expect(parentOf("/home/ana/notes/")).toBe("/home/ana");
  });

  it("keeps the root of a drive and of the file system", () => {
    expect(parentOf("F:\\work")).toBe("F:\\");
    expect(parentOf("/home")).toBe("/");
  });

  it("is empty for a name with no folder", () => {
    expect(parentOf("notes")).toBe("");
  });
});

describe("parentLabel", () => {
  it("is the whole parent folder when it is short", () => {
    expect(parentLabel("F:\\work\\big-repo")).toBe("F:\\work");
    expect(parentLabel("F:\\work")).toBe("F:\\");
    expect(parentLabel("/home/ana")).toBe("/home");
  });

  it("keeps only the name of a deep one, in the separators the path has", () => {
    expect(parentLabel("C:\\Users\\ana\\AppData\\projects\\team-b\\docs")).toBe("…\\team-b");
    expect(parentLabel("/home/ana/code/projects/docs")).toBe("…/projects");
    expect(parentLabel("\\\\server\\share\\team\\docs")).toBe("…\\team");
  });
});

describe("switcherRows", () => {
  const recents = [
    recent("F:\\work\\alpha", "alpha"),
    recent("F:\\work\\beta", "beta"),
    recent("D:\\old\\gamma", "gamma"),
  ];

  it("lists the folder of this window first, marked, and the others in the order of the list", () => {
    const rows = switcherRows({
      recents,
      hidden: NONE,
      missing: NONE,
      current: { root: "F:\\work\\beta", name: "beta" },
    });
    expect(rows.map((r) => [r.name, r.current])).toEqual([
      ["beta", true],
      ["alpha", false],
      ["gamma", false],
    ]);
    expect(rows[1].parent).toBe("F:\\work");
  });

  it("lists the folder of this window even when it is not in the list yet", () => {
    const rows = switcherRows({
      recents,
      hidden: NONE,
      missing: NONE,
      current: { root: "G:\\fresh", name: "fresh" },
    });
    expect(rows.map((r) => r.name)).toEqual(["fresh", "alpha", "beta", "gamma"]);
    expect(rows[0].current).toBe(true);
  });

  it("lists the folder of this window once, whatever way its path is written", () => {
    const rows = switcherRows({
      recents,
      hidden: NONE,
      missing: NONE,
      current: { root: "f:/work/ALPHA/", name: "alpha" },
    });
    expect(rows.filter((r) => r.name === "alpha")).toHaveLength(1);
    expect(rows[0].current).toBe(true);
  });

  it("leaves out the folders the person removed, but never the one on screen", () => {
    const hidden = new Set(["F:\\work\\alpha", "D:\\old\\gamma"]);
    const rows = switcherRows({
      recents,
      hidden,
      missing: NONE,
      current: { root: "F:\\work\\alpha", name: "alpha" },
    });
    expect(rows.map((r) => r.name)).toEqual(["alpha", "beta"]);
  });

  it("marks the folders that were not found, and never the one on screen", () => {
    const rows = switcherRows({
      recents,
      hidden: NONE,
      missing: new Set(["D:\\old\\gamma", "F:\\work\\alpha"]),
      current: { root: "F:\\work\\alpha", name: "alpha" },
    });
    expect(rows.map((r) => [r.name, r.missing])).toEqual([
      ["alpha", false],
      ["beta", false],
      ["gamma", true],
    ]);
  });

  it("is only the list when no folder is open", () => {
    const rows = switcherRows({ recents, hidden: NONE, missing: NONE, current: null });
    expect(rows.map((r) => r.name)).toEqual(["alpha", "beta", "gamma"]);
    expect(rows.some((r) => r.current)).toBe(false);
  });
});

describe("switchable", () => {
  it("is every row but the current folder and the folders not found", () => {
    const rows = switcherRows({
      recents: [recent("/a", "a"), recent("/b", "b"), recent("/c", "c")],
      hidden: NONE,
      missing: new Set(["/c"]),
      current: { root: "/a", name: "a" },
    });
    expect(switchable(rows).map((r) => r.name)).toEqual(["b"]);
  });
});
