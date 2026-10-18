import { describe, expect, it } from "vitest";
import {
  contextAt,
  fileWords,
  headingIds,
  headingSlug,
  linkSuggestions,
  relativeLink,
  suggestWords,
  wordsOf,
} from "./complete";

const files = ["docs/a.md", "docs/b.md", "docs/my file.md", "other/b-extra.md", "img/logo.png"];

function links(typed: string, wrap = true) {
  return linkSuggestions(typed, {
    fromFile: "docs/a.md",
    files,
    headings: ["Top", "Café & Chá", "Descrição dos Personagens", "Descrição dos Personagens"],
    wrap,
  });
}

describe("heading fragments", () => {
  it("drops punctuation and keeps letters the way the page does", () => {
    expect(headingSlug("Café & Chá")).toBe("café--chá");
    expect(headingSlug("Descrição dos Personagens")).toBe("descrição-dos-personagens");
    expect(headingSlug("Hello, world")).toBe("hello-world");
  });

  it("numbers a repeated fragment", () => {
    expect(
      headingIds(["Café & Chá", "Descrição dos Personagens", "Descrição dos Personagens"]),
    ).toEqual([
      { text: "Café & Chá", id: "café--chá" },
      { text: "Descrição dos Personagens", id: "descrição-dos-personagens" },
      { text: "Descrição dos Personagens", id: "descrição-dos-personagens-1" },
    ]);
  });
});

describe("paths from the current document", () => {
  it("stays in the folder when it can, and climbs out when it must", () => {
    expect(relativeLink("docs/a.md", "docs/b.md")).toBe("b.md");
    expect(relativeLink("docs/a.md", "img/logo.png")).toBe("../img/logo.png");
    expect(relativeLink("a.md", "docs/b.md")).toBe("docs/b.md");
  });

  it("does not point at itself, another worktree, or a loose file", () => {
    expect(relativeLink("docs/a.md", "docs/a.md")).toBeNull();
    expect(relativeLink("docs/a.md", "@wt/feature/docs/b.md")).toBeNull();
    expect(relativeLink("@wt/feature/docs/a.md", "@wt/feature/docs/b.md")).toBe("b.md");
    expect(relativeLink("C:/notes/a.md", "docs/b.md")).toBeNull();
  });
});

describe("where the caret is", () => {
  it("reads the destination of a link, including one still being typed", () => {
    const text = "See [guide](gui";
    expect(contextAt(text, text.length)).toEqual({
      kind: "link",
      from: text.lastIndexOf("gui"),
      to: text.length,
      query: "gui",
      bracketed: false,
    });
  });

  it("keeps the angle brackets outside the part that is replaced", () => {
    const text = "See [a](<my fi";
    const at = contextAt(text, text.length);
    expect(at).toMatchObject({ kind: "link", query: "my fi", bracketed: true });
  });

  it("treats the part after # as a heading, and a reference definition as a path", () => {
    const linked = "See [a](b.md#To";
    expect(contextAt(linked, linked.length)?.query).toBe("b.md#To");
    const def = "[ref]: gui";
    expect(contextAt(def, def.length)).toMatchObject({ kind: "link", query: "gui" });
  });

  it("does not complete inside a fence, inline code, or an escaped bracket", () => {
    const fenced = "```\nconst guide";
    expect(contextAt(fenced, fenced.length)).toBeNull();
    const code = "use `gui";
    expect(contextAt(code, code.length)).toBeNull();
    const escaped = "not a \\](gui";
    expect(contextAt(escaped, escaped.length)?.kind).toBe("word");
  });

  it("takes the whole word, and a single CJK character", () => {
    const text = "markdown";
    expect(contextAt(text, 4)).toEqual({ kind: "word", from: 0, to: 8, query: "mark" });
    expect(contextAt("See g", 5)).toBeNull();
    expect(contextAt("你", 1)).toMatchObject({ kind: "word", query: "你" });
  });
});

describe("what is offered", () => {
  it("offers the same folder first, and wraps a path that has a space", () => {
    expect(links("b").map((item) => item.insert)).toEqual(["b.md", "../other/b-extra.md"]);
    expect(links("my").map((item) => item.insert)).toEqual(["<my file.md>"]);
    expect(links("my", false).map((item) => item.insert)).toEqual(["my file.md"]);
  });

  it("offers headings of this document after #, and nothing for a web address", () => {
    const headings = links("#").filter((item) => item.kind === "heading");
    expect(headings.map((item) => item.detail)).toEqual([
      "#top",
      "#café--chá",
      "#descrição-dos-personagens",
      "#descrição-dos-personagens-1",
    ]);
    expect(links("#ca")[0]).toMatchObject({ insert: "#café--chá", label: "Café & Chá" });
    expect(links("https://ex")).toEqual([]);
    expect(links("other.md#To")).toEqual([]);
  });

  it("continues a word, and skips the word already typed", () => {
    expect(suggestWords("ma", ["Markdown", "schema", "mark"]).map((item) => item.insert)).toEqual([
      "mark",
      "Markdown",
    ]);
    expect(suggestWords("markdown", ["markdown"])).toEqual([]);
    expect(wordsOf("See the guide\n\n```\nfunction hidden\n```\n`secret` stays")).toEqual([
      "See",
      "the",
      "guide",
      "stays",
    ]);
    expect(wordsOf("你好世界")).toEqual(["你好", "好世", "世界"]);
    expect(fileWords(["docs/saturday-potluck.md"])).toEqual(["saturday", "potluck"]);
  });
});
