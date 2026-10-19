import { describe, expect, it } from "vitest";
import { findTextRefs, parseInlineRef, refLabel } from "./codeRefs";

describe("inline references", () => {
  it("reads a path with or without a line", () => {
    expect(parseInlineRef("src/main.rs")).toEqual({
      path: "src/main.rs",
      line: null,
      endLine: null,
      col: null,
    });
    expect(parseInlineRef("src/main.rs:42")).toMatchObject({ line: 42, endLine: null, col: null });
    expect(parseInlineRef("src/main.rs:42:7")).toMatchObject({ line: 42, col: 7 });
    expect(parseInlineRef("src/main.rs:10-20")).toMatchObject({ line: 10, endLine: 20 });
    expect(parseInlineRef("a/b.ts#L10")).toMatchObject({ line: 10 });
    expect(parseInlineRef("a/b.ts#L10C5")).toMatchObject({ line: 10, col: 5 });
    expect(parseInlineRef("a/b.ts#L10-L20")).toMatchObject({ line: 10, endLine: 20 });
    expect(parseInlineRef("  src/x.rs:3  ")).toMatchObject({ path: "src/x.rs", line: 3 });
  });

  it("takes backslashes, the mention form and a backwards range", () => {
    expect(parseInlineRef("src\\main.rs:1")?.path).toBe("src/main.rs");
    expect(parseInlineRef("@docs/plan.md")?.path).toBe("docs/plan.md");
    expect(parseInlineRef("a.rs:20-10")).toMatchObject({ line: 20, endLine: null });
  });

  it("refuses what is not a file reference", () => {
    for (const text of [
      "",
      "main",
      "1.2",
      "192.168.1.1",
      "v1.0.2",
      "a.rs:0",
      "https://example.com/a.rs:3",
      "C:\\x\\a.rs",
      "two words.rs",
      "a.rs:1x",
      "x".repeat(500) + ".rs",
      "a.toolongextension1234",
    ]) {
      expect(parseInlineRef(text), text).toBeNull();
    }
  });
});

describe("references in text", () => {
  it("needs a line, finds each, and gives exact ranges", () => {
    const text = "See src/main.rs:12 and docs/a.md#L3-L8, not src/plain.rs or v1.2:3.";
    const found = findTextRefs(text);
    expect(found.map((f) => text.slice(f.start, f.end))).toEqual([
      "src/main.rs:12",
      "docs/a.md#L3-L8",
    ]);
    expect(found[0]?.ref).toMatchObject({ path: "src/main.rs", line: 12 });
    expect(found[1]?.ref).toMatchObject({ path: "docs/a.md", line: 3, endLine: 8 });
  });

  it("is not read from the middle of a longer token", () => {
    expect(findTextRefs("https://github.com/o/r/blob/main/src/a.rs:10")).toEqual([]);
    expect(findTextRefs("C:\\proj\\a.rs:10")).toEqual([]);
    expect(findTextRefs("x:src/a.rs:10")).toEqual([]);
    expect(findTextRefs("(src/a.rs:10)")).toHaveLength(1);
    expect(findTextRefs("src/a.rs:10x")).toEqual([]);
  });

  it("stays fast and safe on hostile text", () => {
    const started = Date.now();
    expect(findTextRefs("a:1".repeat(20_000))).toEqual([]);
    expect(findTextRefs(":".repeat(60_000))).toEqual([]);
    expect(findTextRefs(`${"a/".repeat(5_000)}b.rs:1`)).toEqual([]);
    expect(findTextRefs("#L1 ".repeat(10_000))).toEqual([]);
    expect(Date.now() - started).toBeLessThan(2000);
  });

  it("writes a reference back", () => {
    expect(refLabel("a.rs", 3)).toBe("a.rs:3");
    expect(refLabel("a.rs", null)).toBe("a.rs");
  });
});
