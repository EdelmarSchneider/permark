import { describe, expect, it } from "vitest";
import {
  MAX_CANDIDATES,
  buildFileIndex,
  findTextRefs,
  parseInlineRef,
  refLabel,
  resolveCodeRef,
  type CodeRef,
} from "./codeRefs";

const ref = (path: string, line: number | null = null, col: number | null = null): CodeRef => ({
  path,
  line,
  endLine: null,
  col,
});

describe("parseInlineRef", () => {
  it("reads a path, with or without a line", () => {
    expect(parseInlineRef("src/main.rs")).toEqual(ref("src/main.rs"));
    expect(parseInlineRef("src/main.rs:42")).toEqual(ref("src/main.rs", 42));
    expect(parseInlineRef("main.rs:42:7")).toEqual(ref("main.rs", 42, 7));
    expect(parseInlineRef("lib.rs#L10")).toEqual(ref("lib.rs", 10));
  });

  it("reads ranges of lines", () => {
    expect(parseInlineRef("lib.rs:10-20")).toEqual({ ...ref("lib.rs", 10), endLine: 20 });
    expect(parseInlineRef("lib.rs#L10-L20")).toEqual({ ...ref("lib.rs", 10), endLine: 20 });
    expect(parseInlineRef("lib.rs#L10C5-L20C8")).toEqual({
      ...ref("lib.rs", 10, 5),
      endLine: 20,
    });
  });

  it("drops a range that runs backwards but keeps the line", () => {
    expect(parseInlineRef("lib.rs:20-10")).toEqual(ref("lib.rs", 20));
  });

  it("reads relative paths, dotfiles and extra dots", () => {
    expect(parseInlineRef("./src/a.ts:3")).toEqual(ref("./src/a.ts", 3));
    expect(parseInlineRef("../shared/util.ts")).toEqual(ref("../shared/util.ts"));
    expect(parseInlineRef(".gitignore")).toEqual(ref(".gitignore"));
    expect(parseInlineRef("src/app.test.tsx:9")).toEqual(ref("src/app.test.tsx", 9));
    expect(parseInlineRef("docs/design/v0.3.md")).toEqual(ref("docs/design/v0.3.md"));
    expect(parseInlineRef("/src/main.rs:5")).toEqual(ref("/src/main.rs", 5));
    expect(parseInlineRef("src/data@2x.png")).toEqual(ref("src/data@2x.png"));
  });

  it("reads a mention as 'Copy reference' writes it, and a worktree path as it is", () => {
    expect(parseInlineRef("@docs/a.md")).toEqual(ref("docs/a.md"));
    expect(parseInlineRef("@src/main.rs:42")).toEqual(ref("src/main.rs", 42));
    expect(parseInlineRef("@wt/feat/src/main.rs:42")).toEqual(ref("@wt/feat/src/main.rs", 42));
  });

  it("turns backslashes into slashes", () => {
    expect(parseInlineRef("src\\lib\\main.rs:42")).toEqual(ref("src/lib/main.rs", 42));
  });

  it("ignores the whitespace around the code", () => {
    expect(parseInlineRef("  src/main.rs:42 ")).toEqual(ref("src/main.rs", 42));
  });

  it("is not fooled by things that are not references", () => {
    for (const text of [
      "",
      "   ",
      "version 1:2",
      "10:30",
      "1:2",
      "main",
      "src/",
      "src/main.rs:",
      "src/main.rs:abc",
      "src/main.rs:0",
      "src/main.rs#L0",
      "src/main.rs#10",
      "src/main.rs:1:2:3",
      "a b.rs",
      "see src/main.rs",
      "src/*.rs",
      "main.rs()",
      "foo.bar(baz)",
      "v1.2",
      "3.14",
      "192.168.1.1",
      "192.168.1.1:8080",
      "1.2.3",
      "x".repeat(500) + ".rs",
    ]) {
      expect(parseInlineRef(text), JSON.stringify(text)).toBeNull();
    }
  });

  it("does not read URLs, drive paths or network paths", () => {
    for (const text of [
      "https://example.com/src/main.rs:42",
      "http://localhost:3000/app.js",
      "file:///home/me/main.rs",
      "C:\\Users\\me\\proj\\src\\main.rs:42",
      "C:/Users/me/main.rs",
      "d:\\notes.md",
      "\\\\server\\share\\x.rs:3",
      "git@github.com:me/repo.git",
      "mailto:me@example.com",
    ]) {
      expect(parseInlineRef(text), text).toBeNull();
    }
  });

  it("lets a name through that only the project can confirm", () => {
    // looks like a file name; the resolver decides whether the project has it
    expect(parseInlineRef("example.com")).toEqual(ref("example.com"));
    expect(parseInlineRef("Foo.Bar")).toEqual(ref("Foo.Bar"));
  });
});

describe("findTextRefs", () => {
  const found = (text: string): [string, CodeRef][] =>
    findTextRefs(text).map((r) => [text.slice(r.start, r.end), r.ref]);

  it("needs a line: a bare path in prose stays text", () => {
    expect(findTextRefs("Open src/main.rs and read it.")).toEqual([]);
    expect(findTextRefs("See docs/design/v0.3.md for details")).toEqual([]);
  });

  it("finds a reference and reports where it is", () => {
    const text = "The bug is in src/main.rs:42 near the loop.";
    expect(findTextRefs(text)).toEqual([{ start: 14, end: 28, ref: ref("src/main.rs", 42) }]);
    expect(text.slice(14, 28)).toBe("src/main.rs:42");
  });

  it("finds every reference of a text", () => {
    expect(found("a/b.rs:1, c/d.ts:22:3 and e.md#L5-L9")).toEqual([
      ["a/b.rs:1", ref("a/b.rs", 1)],
      ["c/d.ts:22:3", ref("c/d.ts", 22, 3)],
      ["e.md#L5-L9", { ...ref("e.md", 5), endLine: 9 }],
    ]);
  });

  it("stops before the punctuation of the sentence", () => {
    expect(found("See src/main.rs:42.")).toEqual([["src/main.rs:42", ref("src/main.rs", 42)]]);
    expect(found("(src/main.rs:42)")).toEqual([["src/main.rs:42", ref("src/main.rs", 42)]]);
    expect(found('"src/main.rs:42"')).toEqual([["src/main.rs:42", ref("src/main.rs", 42)]]);
    expect(found("src/main.rs:42, then")).toEqual([["src/main.rs:42", ref("src/main.rs", 42)]]);
  });

  it("reads the location of a compiler message", () => {
    expect(found("src/main.rs:42:7: error: unused")).toEqual([
      ["src/main.rs:42:7", ref("src/main.rs", 42, 7)],
    ]);
  });

  it("reads a path with backslashes", () => {
    expect(found("at src\\lib\\main.rs:42")).toEqual([
      ["src\\lib\\main.rs:42", ref("src/lib/main.rs", 42)],
    ]);
  });

  it("leaves prose alone", () => {
    for (const text of [
      "version 1:2",
      "Release 3.12:5 is out",
      "The meeting is at 10:30 on Monday",
      "Ratio 16:9 and 4:3",
      "Note: this matters",
      "Step 1: do it",
      "e.g.:3 items",
      "main.rs:0",
      "main.rs:42abc",
      "a title with a #L10 anchor",
      "no colon here",
      "",
    ]) {
      expect(findTextRefs(text), text).toEqual([]);
    }
  });

  it("does not take a reference out of the middle of a URL", () => {
    for (const text of [
      "https://example.com/src/main.rs:42",
      "see http://localhost:3000/a/b.js:12 now",
      "https://github.com/me/repo/blob/main/src/lib.rs#L10-L20",
      "ftp://host/file.txt:5",
      "x=https://a.b/c.rs:3",
    ]) {
      expect(findTextRefs(text), text).toEqual([]);
    }
  });

  it("does not take a reference out of the middle of a drive or network path", () => {
    for (const text of [
      "C:\\Users\\me\\proj\\src\\main.rs:42",
      "see D:\\code\\a.ts:7 for it",
      "C:/Users/me/main.rs:42",
      "\\\\server\\share\\x.rs:3",
    ]) {
      expect(findTextRefs(text), text).toEqual([]);
    }
  });

  it("does not start in the middle of a longer word", () => {
    expect(findTextRefs("xsrc/main.rs:42")).toEqual([
      { start: 0, end: 15, ref: ref("xsrc/main.rs", 42) },
    ]);
    expect(findTextRefs("path=src/main.rs:42")).toEqual([]);
    // a mention as "Copy reference" writes it: the @ goes with the link, not with the path
    expect(findTextRefs("@src/main.rs:42")).toEqual([
      { start: 0, end: 15, ref: ref("src/main.rs", 42) },
    ]);
    expect(findTextRefs("me@example.com:25 or a@b.rs:3")).toEqual([
      { start: 0, end: 17, ref: ref("me@example.com", 25) },
      { start: 21, end: 29, ref: ref("a@b.rs", 3) },
    ]);
  });

  it("is quick on a long run of name characters", () => {
    const text = `${"a".repeat(20_000)}:5`;
    const started = performance.now();
    expect(findTextRefs(text)).toEqual([]);
    expect(performance.now() - started).toBeLessThan(2_000);
  });

  it("does not scan a huge text", () => {
    expect(findTextRefs(`src/main.rs:42 ${"x ".repeat(30_000)}`)).toEqual([]);
  });

  it("can be used again and again (the pattern keeps no state)", () => {
    expect(findTextRefs("a.rs:1")).toHaveLength(1);
    expect(findTextRefs("a.rs:1")).toHaveLength(1);
    expect(findTextRefs("b.rs:2 c.rs:3")).toHaveLength(2);
  });
});

describe("resolveCodeRef", () => {
  const index = buildFileIndex([
    "README.md",
    "src/main.rs",
    "src/lib/util.ts",
    "docs/guide.md",
    "docs/src/local.rs",
    "docs/design/v0.3.md",
    "pkg/a/index.ts",
    "pkg/b/index.ts",
    "docs/index.ts",
    "@wt/feat/src/main.rs",
    "@wt/feat/src/only-here.rs",
    "@wt/feat/docs/a.md",
    "@wt/other/src/main.rs",
  ]);
  const resolve = (doc: string, written: string): string[] => resolveCodeRef(index, doc, written);

  it("finds a path from the project root", () => {
    expect(resolve("docs/guide.md", "src/main.rs")).toEqual(["src/main.rs"]);
    expect(resolve("README.md", "docs/design/v0.3.md")).toEqual(["docs/design/v0.3.md"]);
  });

  it("takes the root before the document's folder", () => {
    // `src/main.rs` exists at the root; `docs/src/main.rs` would be the folder's, and does not exist
    expect(resolve("docs/guide.md", "src/main.rs")).toEqual(["src/main.rs"]);
  });

  it("finds a path from the document's folder", () => {
    expect(resolve("docs/guide.md", "src/local.rs")).toEqual(["docs/src/local.rs"]);
    expect(resolve("docs/design/v0.3.md", "../guide.md")).toEqual(["docs/guide.md"]);
    expect(resolve("docs/guide.md", "./src/local.rs")).toEqual(["docs/src/local.rs"]);
  });

  it("finds a file by its name when the name is unique", () => {
    expect(resolve("docs/guide.md", "util.ts")).toEqual(["src/lib/util.ts"]);
    expect(resolve("docs/guide.md", "v0.3.md")).toEqual(["docs/design/v0.3.md"]);
  });

  it("finds a file by the end of its path", () => {
    expect(resolve("README.md", "lib/util.ts")).toEqual(["src/lib/util.ts"]);
    expect(resolve("README.md", "design/v0.3.md")).toEqual(["docs/design/v0.3.md"]);
    // a `../` that points nowhere from here still names the end of a path
    expect(resolve("README.md", "../lib/util.ts")).toEqual(["src/lib/util.ts"]);
  });

  it("lists every candidate when the name is shared, the closest to the document first", () => {
    expect(resolve("pkg/c/notes.md", "index.ts")).toEqual([
      "pkg/a/index.ts",
      "pkg/b/index.ts",
      "docs/index.ts",
    ]);
    // the document's own folder settles it
    expect(resolve("pkg/b/notes.md", "index.ts")).toEqual(["pkg/b/index.ts"]);
    expect(resolve("docs/guide.md", "index.ts")).toEqual(["docs/index.ts"]);
    // nothing is closer than anything else to a document at the root: by name
    expect(resolve("README.md", "index.ts")).toEqual([
      "docs/index.ts",
      "pkg/a/index.ts",
      "pkg/b/index.ts",
    ]);
  });

  it("leaves a name that matches too many files as plain text", () => {
    const many = buildFileIndex(
      Array.from({ length: MAX_CANDIDATES + 1 }, (_, i) => `pkg${i}/mod.rs`),
    );
    expect(resolveCodeRef(many, "README.md", "mod.rs")).toEqual([]);
    const fits = buildFileIndex(Array.from({ length: MAX_CANDIDATES }, (_, i) => `p${i}/mod.rs`));
    expect(resolveCodeRef(fits, "README.md", "mod.rs")).toHaveLength(MAX_CANDIDATES);
  });

  it("finds nothing for a file the project does not have", () => {
    expect(resolve("README.md", "src/missing.rs")).toEqual([]);
    expect(resolve("README.md", "nothing.txt")).toEqual([]);
    expect(resolve("README.md", "example.com")).toEqual([]);
  });

  it("finds nothing for a folder", () => {
    expect(resolve("README.md", "src/lib")).toEqual([]);
    expect(resolve("README.md", "docs.d")).toEqual([]);
  });

  it("does not look outside the project", () => {
    expect(resolve("README.md", "../outside.rs")).toEqual([]);
    expect(resolve("README.md", "../../etc/passwd.txt")).toEqual([]);
    expect(resolve("README.md", "/home/me/proj/src/main.rs")).toEqual([]);
  });

  it("reads a leading slash as the project root", () => {
    expect(resolve("docs/guide.md", "/src/main.rs")).toEqual(["src/main.rs"]);
  });

  it("reads backslashes as folders", () => {
    expect(resolve("docs/guide.md", "src\\lib\\util.ts")).toEqual(["src/lib/util.ts"]);
  });

  it("keeps a document of a worktree inside that worktree", () => {
    expect(resolve("@wt/feat/docs/a.md", "src/main.rs")).toEqual(["@wt/feat/src/main.rs"]);
    // the main folder has `src/lib/util.ts`, the worktree does not: no link into another checkout
    expect(resolve("@wt/feat/docs/a.md", "src/lib/util.ts")).toEqual([]);
    expect(resolve("@wt/feat/docs/a.md", "only-here.rs")).toEqual(["@wt/feat/src/only-here.rs"]);
    expect(resolve("@wt/feat/docs/a.md", "main.rs")).toEqual(["@wt/feat/src/main.rs"]);
  });

  it("never looks into the worktrees from the main folder", () => {
    // `main.rs` is in the main folder and in two worktrees; only the main one counts
    expect(resolve("README.md", "main.rs")).toEqual(["src/main.rs"]);
    expect(resolve("README.md", "only-here.rs")).toEqual([]);
  });

  it("follows a path that names the worktree", () => {
    expect(resolve("README.md", "@wt/feat/src/main.rs")).toEqual(["@wt/feat/src/main.rs"]);
  });
});

describe("refLabel", () => {
  it("writes the path with its line", () => {
    expect(refLabel("src/main.rs", 42)).toBe("src/main.rs:42");
    expect(refLabel("src/main.rs", null)).toBe("src/main.rs");
  });
});
