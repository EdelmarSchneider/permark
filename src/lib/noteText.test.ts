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

  it("counts each Chinese character and each Japanese kana or kanji as one word", () => {
    expect(countWords("购物清单")).toBe(4);
    // the punctuation between the characters is not a word
    expect(countWords("购物清单：苹果、牛奶、面包。")).toBe(10);
    expect(countWords("ありがとう")).toBe(5);
    expect(countWords("テスト")).toBe(3);
    expect(countWords("コーヒー")).toBe(4);
    expect(countWords("日本語のテキスト")).toBe(8);
    // a paragraph without blanks is its characters, not one run
    expect(countWords("这是第一段。\n\n这是第二段，有一个 空格。")).toBe(5 + 5 + 3 + 2);
  });

  it("adds the characters and the blank-separated words of a mixed text", () => {
    expect(countWords("使用Rust编写")).toBe(5);
    expect(countWords("使用 Rust 编写")).toBe(5);
    expect(countWords("# 标题\n\n第12行 and more")).toBe(2 + 2 + 1 + 2);
    expect(countWords("日本語 and 中文")).toBe(6);
  });

  it("counts Korean as blank-separated words", () => {
    expect(countWords("안녕하세요 세계")).toBe(2);
  });
});

describe("isBlank", () => {
  it("is true for blank characters only, a byte order mark included", () => {
    expect(isBlank("")).toBe(true);
    expect(isBlank(" \r\n\t﻿")).toBe(true);
    expect(isBlank("  x ")).toBe(false);
  });
});
