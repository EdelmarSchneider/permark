import { describe, expect, it } from "vitest";
import { countWords, isBlank, noteTitle } from "./noteText";

describe("noteTitle", () => {
  it("is the first line without its Markdown marks", () => {
    expect(noteTitle("# Server IPs and ports\n\napi 10.0.0.4")).toBe("Server IPs and ports");
    expect(noteTitle("## **Bold** and _lean_ `code`")).toBe("Bold and lean code");
    expect(noteTitle("> a quote")).toBe("a quote");
    expect(noteTitle("- [ ] buy milk")).toBe("buy milk");
    expect(noteTitle("1. first\n2. second")).toBe("first");
    expect(noteTitle("[the docs](https://example.com/x) are here")).toBe("the docs are here");
    expect(noteTitle("![a diagram](img/a.png) caption")).toBe("a diagram caption");
    expect(noteTitle("snake_case_name stays")).toBe("snake_case_name stays");
    expect(noteTitle("a <b>bold</b> word")).toBe("a bold word");
  });

  it("is empty for a note with no text, and skips what says nothing about it", () => {
    expect(noteTitle("")).toBe("");
    expect(noteTitle("  \n\t\n")).toBe("");
    expect(noteTitle("---\n***\n")).toBe("");
    expect(noteTitle("\n\n   \n# After blanks")).toBe("After blanks");
    expect(noteTitle("---\ntitle: x\n---\n\n# Real title")).toBe("Real title");
    expect(noteTitle("﻿# With a BOM")).toBe("With a BOM");
    expect(noteTitle("```\ncode\n```")).toBe("code");
  });

  it("is cut to a sane length", () => {
    expect(noteTitle("x".repeat(500))).toHaveLength(120);
  });
});

describe("countWords", () => {
  it("counts runs of characters between blanks", () => {
    expect(countWords("")).toBe(0);
    expect(countWords("   \n ")).toBe(0);
    expect(countWords("one")).toBe(1);
    expect(countWords("Two words\n\nand a few more\twords")).toBe(7);
  });

  it("does not count the marks of the Markdown that stand alone", () => {
    expect(countWords("# Title\n\n- [ ] one\n- [x] two\n\n---\n\n| a | b |")).toBe(6);
  });
});

describe("isBlank", () => {
  it("is true for blank characters only, a byte order mark included", () => {
    expect(isBlank("")).toBe(true);
    expect(isBlank(" \r\n\t﻿")).toBe(true);
    expect(isBlank("  x ")).toBe(false);
  });
});
