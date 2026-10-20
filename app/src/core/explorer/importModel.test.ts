import { describe, expect, it } from "vitest";
import type { ImportOutcome, ImportPlan } from "../../api";
import { language } from "../i18n";
import {
  addedAnything,
  baseName,
  canMove,
  conflictNames,
  folderLabel,
  isPicturePath,
  nothingText,
  outcomeText,
  pictureType,
  refusalText,
  routeDrop,
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

describe("routeDrop", () => {
  it("puts pictures into an editor that is being edited and imports everything else", () => {
    expect(routeDrop(["a.png", "b.JPG"], true)).toEqual({ kind: "pictures" });
    expect(routeDrop(["a.png", "b.JPG"], false)).toEqual({ kind: "import" });
    expect(routeDrop(["a.png", "notes.md"], true)).toEqual({ kind: "import" });
    expect(routeDrop([], true)).toEqual({ kind: "import" });
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

  it("words a drop where nothing could come in, three reasons at most", () => {
    const refused = ["a", "b", "c", "d"].map((name) => ({
      source: `C:/x/${name}`,
      name,
      reason: "missing" as const,
    }));
    const text = nothingText(plan({ refused }));
    expect(text).toContain("a isn't there any more");
    expect(text).toContain("c isn't there any more");
    expect(text).not.toContain("d isn't");
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
