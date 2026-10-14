import { describe, expect, it } from "vitest";
import {
  UndoStack,
  dropFolder,
  dropProblem,
  entryNameProblem,
  extendSelection,
  isInside,
  isRename,
  isRootPath,
  joinPath,
  linkDecision,
  mapPath,
  mapSet,
  movesFor,
  normalizeLinkUpdate,
  rangeBetween,
  renameSelection,
  renamedPath,
  reversed,
  rootOf,
  selectAfterClick,
  shouldScanLinks,
  targetsOf,
  topLevelOnly,
} from "./fileOps";
import type { MovePreview } from "./types";

const preview = (linkCount: number): MovePreview => ({
  planId: 1,
  moves: [],
  linkCount,
  fileCount: linkCount > 0 ? 1 : 0,
  files: [],
  truncated: false,
  skipped: [],
});

describe("the link update setting", () => {
  it("is Ask unless the stored value is one of the three", () => {
    expect(normalizeLinkUpdate(undefined)).toBe("ask");
    expect(normalizeLinkUpdate(null)).toBe("ask");
    expect(normalizeLinkUpdate("sometimes")).toBe("ask");
    expect(normalizeLinkUpdate(7)).toBe("ask");
    for (const value of ["ask", "always", "never"] as const) {
      expect(normalizeLinkUpdate(value)).toBe(value);
    }
  });

  it("decides what to do once the links are known", () => {
    expect(linkDecision("ask", preview(3))).toBe("ask");
    expect(linkDecision("always", preview(3))).toBe("update");
    expect(linkDecision("never", preview(3))).toBe("skip");
    // nothing to rewrite: no question, whatever the setting
    expect(linkDecision("ask", preview(0))).toBe("update");
    expect(linkDecision("always", preview(0))).toBe("update");
    expect(linkDecision("never", preview(0))).toBe("skip");
  });

  it("never looks at the documents when links are not updated", () => {
    expect(shouldScanLinks("never")).toBe(false);
    expect(shouldScanLinks("ask")).toBe(true);
    expect(shouldScanLinks("always")).toBe(true);
  });
});

describe("paths", () => {
  it("joins and renames", () => {
    expect(joinPath("", "a.md")).toBe("a.md");
    expect(joinPath("docs", "a.md")).toBe("docs/a.md");
    expect(renamedPath("docs/old.md", "new.md")).toBe("docs/new.md");
    expect(renamedPath("old.md", "new.md")).toBe("new.md");
    expect(renamedPath("@wt/feat/a.md", "b.md")).toBe("@wt/feat/b.md");
  });

  it("knows the root of a path", () => {
    expect(rootOf("docs/a.md")).toBe("");
    expect(rootOf("@wt/feat/docs/a.md")).toBe("@wt/feat");
    expect(rootOf("@wt/feat")).toBe("@wt/feat");
    expect(isRootPath("")).toBe(true);
    expect(isRootPath("@wt/feat")).toBe(true);
    expect(isRootPath("@wt/feat/a.md")).toBe(false);
    expect(isRootPath("docs")).toBe(false);
  });

  it("compares whole segments", () => {
    expect(isInside("docs/a.md", "docs")).toBe(true);
    expect(isInside("docs", "docs")).toBe(true);
    expect(isInside("docs-old/a.md", "docs")).toBe(false);
  });

  it("keeps only the paths that are not inside another one", () => {
    expect(topLevelOnly(["docs/a.md", "docs", "img/x.png", "docs/sub/b.md", "img/x.png"])).toEqual([
      "docs",
      "img/x.png",
    ]);
    expect(topLevelOnly([])).toEqual([]);
  });
});

describe("moving by drag and drop", () => {
  it("keeps the name of what is dropped", () => {
    expect(movesFor(["a.md", "docs/b.md"], "archive")).toEqual([
      { from: "a.md", to: "archive/a.md" },
      { from: "docs/b.md", to: "archive/b.md" },
    ]);
    expect(movesFor(["docs/b.md"], "")).toEqual([{ from: "docs/b.md", to: "b.md" }]);
    // a folder and what is inside it move once
    expect(movesFor(["docs", "docs/a.md"], "x")).toEqual([{ from: "docs", to: "x/docs" }]);
  });

  it("refuses the drops that make no sense", () => {
    expect(dropProblem(["docs"], "docs")).toBe("inItself");
    expect(dropProblem(["docs"], "docs/sub")).toBe("inItself");
    expect(dropProblem(["docs/a.md"], "docs")).toBe("samePlace");
    expect(dropProblem(["docs/a.md", "docs/b.md"], "docs")).toBe("samePlace");
    expect(dropProblem(["a.md"], "")).toBe("samePlace");
    expect(dropProblem(["a.md"], "@wt/feat")).toBe("otherRoot");
    expect(dropProblem(["@wt/feat/a.md"], "")).toBe("otherRoot");
    expect(dropProblem(["@wt/feat"], "docs")).toBe("root");
    expect(dropProblem([], "docs")).toBe("notFolder");
  });

  it("accepts a drop when at least one item changes place", () => {
    expect(dropProblem(["a.md"], "docs")).toBeNull();
    expect(dropProblem(["docs/a.md", "b.md"], "docs")).toBeNull();
    expect(dropProblem(["docs"], "archive")).toBeNull();
    expect(dropProblem(["@wt/feat/a.md"], "@wt/feat/docs")).toBeNull();
    expect(dropProblem(["docs-old"], "docs")).toBeNull();
  });

  it("drops on a file land in the folder of that file", () => {
    expect(dropFolder({ path: "docs", isDir: true })).toBe("docs");
    expect(dropFolder({ path: "docs/a.md", isDir: false })).toBe("docs");
    expect(dropFolder({ path: "a.md", isDir: false })).toBe("");
  });
});

describe("following a move", () => {
  const moves = [
    { from: "docs", to: "manual" },
    { from: "README.md", to: "docs-readme.md" },
  ];

  it("maps the moved item and everything inside it", () => {
    expect(mapPath("docs", moves)).toBe("manual");
    expect(mapPath("docs/sub/a.md", moves)).toBe("manual/sub/a.md");
    expect(mapPath("README.md", moves)).toBe("docs-readme.md");
  });

  it("leaves everything else alone", () => {
    expect(mapPath("docs-old/a.md", moves)).toBe("docs-old/a.md");
    expect(mapPath("img/x.png", moves)).toBe("img/x.png");
  });

  it("uses the most specific move", () => {
    const nested = [
      { from: "a", to: "b" },
      { from: "a/x", to: "c" },
    ];
    expect(mapPath("a/x/y.md", nested)).toBe("c/y.md");
    expect(mapPath("a/z.md", nested)).toBe("b/z.md");
  });

  it("maps a set and reverses a list of moves", () => {
    expect([...mapSet(["docs", "docs/sub", "img"], moves)]).toEqual([
      "manual",
      "manual/sub",
      "img",
    ]);
    expect(reversed(moves)).toEqual([
      { from: "docs-readme.md", to: "README.md" },
      { from: "manual", to: "docs" },
    ]);
  });

  it("tells a rename from a move", () => {
    expect(isRename([{ from: "docs/a.md", to: "docs/b.md" }])).toBe(true);
    expect(isRename([{ from: "docs/a.md", to: "other/a.md" }])).toBe(false);
    expect(
      isRename([
        { from: "a.md", to: "b.md" },
        { from: "c.md", to: "d.md" },
      ]),
    ).toBe(false);
  });
});

describe("names", () => {
  const siblings = ["Guide.md", "api.md", "img"];

  it("accepts a good name", () => {
    expect(entryNameProblem("manual.md", siblings)).toBeNull();
    expect(entryNameProblem("notes", siblings)).toBeNull();
    expect(entryNameProblem(".editorconfig", siblings)).toBeNull();
  });

  it("explains what is wrong", () => {
    expect(entryNameProblem("  ", siblings)).toBe("empty");
    expect(entryNameProblem("a/b.md", siblings)).toBe("invalidChars");
    expect(entryNameProblem("a:b", siblings)).toBe("invalidChars");
    expect(entryNameProblem("name.", siblings)).toBe("invalidEnd");
    expect(entryNameProblem(" lead", siblings)).toBe("invalidEnd");
    expect(entryNameProblem("CON.md", siblings)).toBe("reserved");
    expect(entryNameProblem("x".repeat(256), siblings)).toBe("tooLong");
    expect(entryNameProblem("API.md", siblings)).toBe("exists");
    expect(entryNameProblem("IMG", siblings)).toBe("exists");
  });

  it("lets an entry keep its own name, and change its case", () => {
    expect(entryNameProblem("api.md", siblings, "api.md")).toBeNull();
    expect(entryNameProblem("API.md", siblings, "api.md")).toBeNull();
    // but not take the name of another one
    expect(entryNameProblem("guide.md", siblings, "api.md")).toBe("exists");
  });

  it("selects the stem of a file and the whole name of a folder", () => {
    expect(renameSelection("guide.md", false)).toEqual([0, 5]);
    expect(renameSelection("archive.tar.gz", false)).toEqual([0, 11]);
    expect(renameSelection("notes", false)).toEqual([0, 5]);
    expect(renameSelection(".env", false)).toEqual([0, 4]);
    expect(renameSelection("v1.2", true)).toEqual([0, 4]);
  });
});

describe("selection", () => {
  const rows = ["a", "b", "c", "d", "e"];
  const none = { ctrl: false, shift: false };

  it("a plain click selects one row and may open it", () => {
    const out = selectAfterClick(rows, new Set(["a", "b"]), "a", "c", none);
    expect([...out.selection]).toEqual(["c"]);
    expect(out.anchor).toBe("c");
    expect(out.plain).toBe(true);
  });

  it("Ctrl+click toggles a row and moves the anchor", () => {
    let out = selectAfterClick(rows, new Set(["a"]), "a", "c", { ctrl: true, shift: false });
    expect([...out.selection]).toEqual(["a", "c"]);
    expect(out.plain).toBe(false);
    out = selectAfterClick(rows, out.selection, out.anchor, "a", { ctrl: true, shift: false });
    expect([...out.selection]).toEqual(["c"]);
  });

  it("Shift+click selects from the anchor, whichever way", () => {
    let out = selectAfterClick(rows, new Set(["b"]), "b", "d", { ctrl: false, shift: true });
    expect([...out.selection]).toEqual(["b", "c", "d"]);
    expect(out.anchor).toBe("b");
    out = selectAfterClick(rows, out.selection, out.anchor, "a", { ctrl: false, shift: true });
    expect([...out.selection]).toEqual(["a", "b"]);
  });

  it("Ctrl+Shift+click adds the range to what is selected", () => {
    const out = selectAfterClick(rows, new Set(["a"]), "c", "e", { ctrl: true, shift: true });
    expect([...out.selection].sort()).toEqual(["a", "c", "d", "e"]);
  });

  it("Shift+click without an anchor starts at the clicked row", () => {
    const out = selectAfterClick(rows, new Set(), null, "c", { ctrl: false, shift: true });
    expect([...out.selection]).toEqual(["c"]);
  });

  it("ranges follow the order of the rows", () => {
    expect(rangeBetween(rows, "d", "b")).toEqual(["b", "c", "d"]);
    expect(rangeBetween(rows, "x", "b")).toEqual(["b"]);
    expect(rangeBetween(rows, "a", "x")).toEqual([]);
    expect([...extendSelection(rows, "b", "e")]).toEqual(["b", "c", "d", "e"]);
  });

  it("acts on the whole selection when the row is part of it", () => {
    const selection = new Set(["a", "c"]);
    expect(targetsOf(selection, "c").sort()).toEqual(["a", "c"]);
    expect(targetsOf(selection, "b")).toEqual(["b"]);
    expect(targetsOf(new Set(["a"]), "a")).toEqual(["a"]);
  });
});

describe("undo stack", () => {
  const entry = (opId: number) => ({ opId, moves: [], folders: [] });

  it("gives back the newest operation first", () => {
    const stack = new UndoStack();
    stack.push(entry(1));
    stack.push(entry(2));
    expect(stack.peek()?.opId).toBe(2);
    expect(stack.pop()?.opId).toBe(2);
    expect(stack.pop()?.opId).toBe(1);
    expect(stack.pop()).toBeNull();
  });

  it("forgets the oldest beyond its limit", () => {
    const stack = new UndoStack(2);
    for (const id of [1, 2, 3]) stack.push(entry(id));
    expect(stack.size).toBe(2);
    expect(stack.pop()?.opId).toBe(3);
    expect(stack.pop()?.opId).toBe(2);
  });

  it("removes one operation by id", () => {
    const stack = new UndoStack();
    for (const id of [1, 2, 3]) stack.push(entry(id));
    expect(stack.remove(2)?.opId).toBe(2);
    expect(stack.remove(2)).toBeNull();
    expect(stack.size).toBe(2);
    stack.clear();
    expect(stack.size).toBe(0);
  });
});
