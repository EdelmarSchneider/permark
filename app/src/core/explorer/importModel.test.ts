import { describe, expect, it } from "vitest";
import type { ImportOutcome, ImportPlan } from "../../api";
import { language } from "../i18n";
import {
  addedAnything,
  badgeOf,
  baseName,
  canMove,
  conflictNames,
  countsText,
  firstOf,
  folderLabel,
  isPicturePath,
  middle,
  openFailedText,
  outcomeText,
  pictureType,
  refusalText,
  routeDrop,
  type DropContext,
  type DropSpot,
} from "./importModel";

const plan = (patch: Partial<ImportPlan> = {}): ImportPlan => ({
  planId: 1,
  target: "docs",
  items: [],
  refused: [],
  ...patch,
});

const outcome = (patch: Partial<ImportOutcome> = {}): ImportOutcome => ({
  target: "docs",
  copied: 0,
  moved: 0,
  files: 0,
  bytes: 0,
  skipped: 0,
  failed: [],
  links: 0,
  leftBehind: 0,
  cancelled: false,
  placed: [],
  undo: null,
  ...patch,
});

describe("pictures", () => {
  it("are known by their names, whatever the case or the folders", () => {
    for (const path of ["C:\\a\\b.PNG", "/x/y.jpeg", "p.jpg", "q.gif", "r.webp", "s.SVG"]) {
      expect(isPicturePath(path), path).toBe(true);
    }
    for (const path of ["a.md", "png", "a.png.exe", "a.pngx", "", "folder.png/readme.txt"]) {
      expect(isPicturePath(path), path).toBe(false);
    }
  });

  it("have a type for the file the editors build", () => {
    expect(pictureType("a/B.JPG")).toBe("image/jpeg");
    expect(pictureType("x.svg")).toBe("image/svg+xml");
    expect(pictureType("x.txt")).toBe("");
    expect(pictureType("no-extension")).toBe("");
  });

  it("names a file by its last part, either separator", () => {
    expect(baseName("C:\\Users\\me\\图 片.png")).toBe("图 片.png");
    expect(baseName("/a/b/c.md")).toBe("c.md");
    expect(baseName("c.md")).toBe("c.md");
    expect(baseName("/a/b/")).toBe("b");
  });
});

const ctx = (
  workspace: DropContext["workspace"],
  notesTake: DropContext["notesTake"] = null,
): DropContext => ({ workspace, notesTake });
const explorer = (folder: string): DropSpot => ({ area: "explorer", folder });
const editor = (pictures = true): DropSpot => ({ area: "editor", pictures });
const other: DropSpot = { area: "other" };

describe("routeDrop", () => {
  it("pictures only, over an editor that takes them: inserted, in every kind of view", () => {
    for (const workspace of ["project", "singleFile", "notes"] as const) {
      expect(routeDrop(editor(), ["a.png", "b.JPG"], ctx(workspace))).toEqual({
        kind: "pictures",
      });
    }
  });

  it("anything but pictures over an editor: not inserted (opens, or the Notes take it)", () => {
    expect(routeDrop(editor(), ["a.png", "notes.md"], ctx("project"))).toEqual({ kind: "open" });
    expect(routeDrop(editor(false), ["a.png"], ctx("singleFile"))).toEqual({ kind: "open" });
    expect(routeDrop(editor(), [], ctx("project"))).toEqual({ kind: "open" });
  });

  it("the Explorer list of a project: into the folder under the pointer", () => {
    expect(routeDrop(explorer("docs"), ["C:/a.md"], ctx("project"))).toEqual({
      kind: "import",
      folder: "docs",
    });
    expect(routeDrop(explorer(""), ["C:/a.png"], ctx("project"))).toEqual({
      kind: "import",
      folder: "",
    });
  });

  it("the Notes tab: loose files for the Notes, the rest as tabs", () => {
    const take = (p: string): boolean => p.endsWith(".md");
    expect(routeDrop(other, ["C:/a.md", "C:/dir", "C:/b.md"], ctx("notes", take))).toEqual({
      kind: "notes",
      loose: ["C:/a.md", "C:/b.md"],
      open: ["C:/dir"],
    });
    // no Notes hook (the plugin's page not there yet): everything opens
    expect(routeDrop(other, ["C:/a.md"], ctx("notes"))).toEqual({
      kind: "notes",
      loose: [],
      open: ["C:/a.md"],
    });
  });

  it("anywhere else opens: Welcome, single file, a project outside its list, the Search view", () => {
    expect(routeDrop(other, ["C:/a"], ctx("welcome"))).toEqual({ kind: "open" });
    expect(routeDrop(other, ["C:/a"], ctx("singleFile"))).toEqual({ kind: "open" });
    expect(routeDrop(other, ["C:/a", "C:/b"], ctx("project"))).toEqual({ kind: "open" });
    // the Explorer only counts in a project (a stale spot elsewhere never imports)
    expect(routeDrop(explorer("docs"), ["C:/a"], ctx("singleFile"))).toEqual({ kind: "open" });
  });
});

describe("the badge", () => {
  it("says what a drop does, by route and count", () => {
    expect(badgeOf({ kind: "import", folder: "docs" }, 3, "docs", false)).toEqual({
      icon: "import",
      text: "Add 3 items to “docs”",
    });
    expect(badgeOf({ kind: "import", folder: "" }, 1, "my project", false).text).toBe(
      "Add 1 item to “my project”",
    );
    expect(badgeOf({ kind: "open" }, 1, "", false)).toEqual({
      icon: "open",
      text: "Open in a new tab",
    });
    expect(badgeOf({ kind: "open" }, 3, "", false).text).toBe("Open 3 items in new tabs");
    expect(badgeOf({ kind: "pictures" }, 2, "", false)).toEqual({
      icon: "pictures",
      text: "Insert 2 pictures here",
    });
    expect(badgeOf({ kind: "notes", loose: ["a"], open: [] }, 1, "", false)).toEqual({
      icon: "notes",
      text: "Open in Notes",
    });
    expect(badgeOf({ kind: "notes", loose: ["a", "b", "c"], open: [] }, 3, "", false).text).toBe(
      "Open 3 items in Notes",
    );
    expect(badgeOf({ kind: "notes", loose: ["a"], open: ["b", "c"] }, 3, "", false).text).toBe(
      "Open 3 items",
    );
    expect(badgeOf({ kind: "notes", loose: [], open: ["d"] }, 1, "", false)).toEqual({
      icon: "open",
      text: "Open in a new tab",
    });
  });

  it("says to wait while an import runs (over the Explorer only)", () => {
    expect(badgeOf({ kind: "import", folder: "docs" }, 2, "docs", true).text).toBe(
      "Wait for the current import to finish.",
    );
    expect(badgeOf({ kind: "open" }, 2, "", true).text).toBe("Open 2 items in new tabs");
  });

  it("shortens a long folder name in the middle, quotes kept", () => {
    const long = "a-very-long-folder-name-that-goes-on-and-on-文档-end";
    const text = badgeOf({ kind: "import", folder: long }, 1, long, false).text;
    expect(text.startsWith("Add 1 item to “a-very")).toBe(true);
    expect(text.endsWith("end”")).toBe(true);
    expect(text).toContain("…");
    expect(Array.from(middle(long)).length).toBe(32);
    expect(middle("short")).toBe("short");
  });
});

describe("the plan", () => {
  const item = (
    name: string,
    conflict: boolean,
    kind: "file" | "folder" = "file",
    inProject = false,
  ) => ({ source: `C:/x/${name}`, name, kind, conflict, inProject }) as const;

  it("lists the taken names, folders with a slash", () => {
    const p = plan({
      items: [item("a.md", true), item("b.md", false), item("pics", true, "folder")],
    });
    expect(conflictNames(p)).toEqual(["a.md", "pics/"]);
  });

  it("can move only what lies outside the project", () => {
    expect(canMove(plan({ items: [item("a.md", false)] }))).toBe(true);
    expect(canMove(plan({ items: [item("a.md", false), item("b.md", false, "file", true)] }))).toBe(
      false,
    );
    expect(canMove(plan())).toBe(true);
  });

  it("names the project folder by the project's name and any other by its own", () => {
    expect(folderLabel("", "my project")).toBe("my project");
    expect(folderLabel("docs/notes", "my project")).toBe("notes");
    expect(folderLabel("@wt/feature", "p")).toBe("feature");
  });

  it("says why an entry was refused, in words", () => {
    for (const reason of [
      "missing",
      "link",
      "special",
      "badName",
      "protected",
      "intoItself",
    ] as const) {
      expect(refusalText(reason).length, reason).toBeGreaterThan(3);
    }
  });

  it("counts files and folders that can come in", () => {
    expect(countsText(plan({ items: [item("a.md", false)] }))).toBe("1 file");
    expect(countsText(plan({ items: [item("a.md", false), item("b.md", false)] }))).toBe("2 files");
    expect(countsText(plan({ items: [item("x", false, "folder")] }))).toBe("1 folder");
    expect(
      countsText(
        plan({ items: [item("a.md", false), item("b.md", true), item("x", false, "folder")] }),
      ),
    ).toBe("2 files and 1 folder");
  });

  it("lists the first names, then how many more", () => {
    expect(firstOf(["a", "b"], 4)).toBe("a, b");
    expect(firstOf(["a", "b", "c", "d", "e", "f"], 4)).toBe("a, b, c, d and 2 more");
    expect(firstOf([], 4)).toBe("");
  });
});

describe("a drop that opened", () => {
  it("words the failures in one notice, the first one named", () => {
    expect(openFailedText([])).toBeNull();
    const error = { kind: "notFound" as const, subject: "C:/x/a.zip", detail: null };
    expect(openFailedText([{ path: "C:/x/a.zip", error }])).toBe("“a.zip” couldn't be opened.");
    const many = openFailedText([
      { path: "C:\\x\\a b.zip", error },
      { path: "C:/y", error },
      { path: "C:/z", error },
    ]);
    expect(many?.startsWith("3 items couldn't be opened. The first was “a b.zip”: ")).toBe(true);
  });
});

describe("the outcome", () => {
  it("says what was copied, moved, skipped, failed, left and stopped", () => {
    expect(outcomeText(outcome({ copied: 1 }), "docs")).toBe("Copied 1 item to “docs”.");
    expect(outcomeText(outcome({ copied: 3 }), "docs")).toBe("Copied 3 items to “docs”.");
    expect(outcomeText(outcome({ moved: 2 }), "docs")).toBe("Moved 2 items to “docs”.");
    expect(outcomeText(outcome({ moved: 2, copied: 1 }), "docs")).toBe(
      "Moved 2 items to “docs”. 1 item was copied; its original stayed.",
    );
    expect(outcomeText(outcome({ copied: 1, skipped: 2 }), "docs")).toContain(
      "2 items were skipped because their names were taken.",
    );
    expect(outcomeText(outcome({ links: 1 }), "docs")).toBe(
      "1 link inside a folder wasn't copied.",
    );
    expect(outcomeText(outcome({ moved: 1, leftBehind: 3 }), "docs")).toContain(
      "3 originals couldn't be removed and stay where they were.",
    );
    expect(outcomeText(outcome({ copied: 1, cancelled: true }), "docs")).toBe(
      "Copied 1 item to “docs”. Stopped. What was already added stays.",
    );
    expect(outcomeText(outcome({ cancelled: true }), "docs")).toBe("Stopped. Nothing was added.");
  });

  it("names the first failure and how many there were", () => {
    const failed = [
      { name: "a.md", error: { kind: "denied" as const, subject: "C:/a.md", detail: null } },
      { name: "b.md", error: { kind: "io" as const, subject: "C:/b.md", detail: null } },
    ];
    const one = outcomeText(outcome({ failed: failed.slice(0, 1) }), "docs");
    expect(one).toContain("a.md couldn't be added");
    const two = outcomeText(outcome({ failed }), "docs");
    expect(two).toContain("2 items couldn't be added. The first was a.md");
  });

  it("is never empty", () => {
    expect(outcomeText(outcome(), "docs")).toBe("Nothing was added.");
  });

  it("offers undo only when something came in", () => {
    expect(addedAnything(outcome())).toBe(false);
    expect(addedAnything(outcome({ skipped: 3 }))).toBe(false);
    expect(addedAnything(outcome({ copied: 1 }))).toBe(true);
    expect(addedAnything(outcome({ moved: 1 }))).toBe(true);
  });

  it("follows the language", async () => {
    await language.choose("de", []);
    expect(outcomeText(outcome({ copied: 2 }), "docs")).toBe("2 Elemente nach „docs“ kopiert.");
    await language.choose("en", []);
  });
});
