import { describe, expect, it } from "vitest";
import { editBlock, editBlockFromKind } from "./editability";
import {
  blankTemplate,
  freshName,
  namesIn,
  nameProblem,
  newFilePath,
  stemRange,
  targetFolder,
  titleOf,
  withExtension,
} from "./newFile";

describe("withExtension", () => {
  it("adds .md when there is no extension", () => {
    expect(withExtension("notes")).toBe("notes.md");
    expect(withExtension("  notes  ")).toBe("notes.md");
    expect(withExtension("notes.")).toBe("notes.md");
    expect(withExtension("v1.2 plan")).toBe("v1.2 plan");
    expect(withExtension("")).toBe("");
  });

  it("keeps the extension that is there", () => {
    expect(withExtension("notes.md")).toBe("notes.md");
    expect(withExtension("notes.txt")).toBe("notes.txt");
    expect(withExtension(".md")).toBe(".md.md");
  });
});

describe("nameProblem", () => {
  const siblings = ["README.md", "notes.md"];

  it("accepts a good name", () => {
    expect(nameProblem("meeting notes 2026.md", siblings)).toBeNull();
    expect(nameProblem("Résumé – draft.markdown", siblings)).toBeNull();
  });

  it("names what is wrong", () => {
    expect(nameProblem("", siblings)).toBe("empty");
    expect(nameProblem("   ", siblings)).toBe("empty");
    for (const bad of ["a?.md", "a:b.md", "a/b.md", "a\\b.md", 'a"b.md', "a<b>.md", "a|b.md"]) {
      expect(nameProblem(bad, siblings), bad).toBe("invalidChars");
    }
    expect(nameProblem("trailing .md.", siblings)).toBe("invalidEnd");
    expect(nameProblem(" leading.md", siblings)).toBe("invalidEnd");
    expect(nameProblem("notes.txt", siblings)).toBe("notMarkdown");
    expect(nameProblem(".md", siblings)).toBe("notMarkdown");
    expect(nameProblem("CON.md", siblings)).toBe("reserved");
    expect(nameProblem("lpt1.md", siblings)).toBe("reserved");
    expect(nameProblem("con .md", siblings)).toBe("reserved");
    expect(nameProblem("Lpt1  .md", siblings)).toBe("reserved");
    expect(nameProblem("console.md", siblings)).toBeNull();
    expect(nameProblem(`${"é".repeat(200)}.md`, siblings)).toBe("tooLong");
  });

  it("says a name exists whatever its case", () => {
    expect(nameProblem("notes.md", siblings)).toBe("exists");
    expect(nameProblem("NOTES.MD", siblings)).toBe("exists");
    expect(nameProblem("readme.md", siblings)).toBe("exists");
  });
});

describe("the name field", () => {
  it("selects the stem", () => {
    expect(stemRange("untitled.md")).toEqual([0, 8]);
    expect(stemRange("a.b.md")).toEqual([0, 3]);
    expect(stemRange("noext")).toEqual([0, 5]);
  });

  it("offers the first free untitled name", () => {
    expect(freshName([])).toBe("untitled.md");
    expect(freshName(["untitled.md"])).toBe("untitled-2.md");
    expect(freshName(["Untitled.md", "untitled-2.md"])).toBe("untitled-3.md");
  });

  it("starts the file with a title made from its name", () => {
    expect(titleOf("meeting-notes_2026.md")).toBe("meeting notes 2026");
    expect(blankTemplate("untitled.md")).toBe("# untitled\n");
    expect(blankTemplate("Release Plan.markdown")).toBe("# Release Plan\n");
  });
});

describe("where the file goes", () => {
  it("uses the selected folder, or the folder of the selected file", () => {
    expect(targetFolder({ path: "docs/specs", isDir: true }, "README.md")).toBe("docs/specs");
    expect(targetFolder({ path: "docs/specs/a.md", isDir: false }, null)).toBe("docs/specs");
    expect(targetFolder({ path: "a.md", isDir: false }, "docs/b.md")).toBe("");
    expect(targetFolder({ path: "@wt/feat", isDir: false }, null)).toBe("@wt/feat");
    expect(targetFolder({ path: "@wt/feat/docs/a.md", isDir: false }, null)).toBe("@wt/feat/docs");
  });

  it("falls back to the open document's folder, then the root", () => {
    expect(targetFolder(null, "docs/specs/live-feed.md")).toBe("docs/specs");
    expect(targetFolder(null, "README.md")).toBe("");
    expect(targetFolder(null, null)).toBe("");
  });

  it("joins folder and name, and lists the names already there", () => {
    expect(newFilePath("docs", "a.md")).toBe("docs/a.md");
    expect(newFilePath("", "a.md")).toBe("a.md");
    expect(namesIn("docs", ["docs/a.md", "docs/sub/b.md", "c.md"])).toEqual(["a.md"]);
    expect(namesIn("", ["docs/a.md", "c.md"])).toEqual(["c.md"]);
  });
});

describe("editBlock", () => {
  it("allows Markdown and says why not for the rest", () => {
    expect(editBlock({ kind: "markdown", encodingNote: null })).toBeNull();
    expect(editBlock({ kind: "binary", encodingNote: null })).toBe("binary");
    expect(editBlock({ kind: "tooLarge", encodingNote: null })).toBe("tooLarge");
    expect(editBlock({ kind: "json", encodingNote: null })).toBe("nonMarkdown");
    expect(editBlock({ kind: "text", encodingNote: "Shown as plain text" })).toBe("nonMarkdown");
    expect(editBlock({ kind: "markdown", encodingNote: "Converted from UTF-16 LE." })).toBe(
      "encoding",
    );
    expect(editBlock(null)).toBeNull();
  });

  it("maps the backend's error kinds to the same reasons", () => {
    expect(editBlockFromKind("notMarkdown")).toBe("nonMarkdown");
    expect(editBlockFromKind("binary")).toBe("binary");
    expect(editBlockFromKind("io")).toBeNull();
    expect(editBlockFromKind(null)).toBeNull();
  });
});
