// The words a spelling check looks at: prose only, with the editors' UTF-16 offsets.
import { describe, expect, it } from "vitest";
import { sampleOf, SAMPLE_CHARS, tokenize } from "./tokenize";

const words = (text: string): string[] => tokenize(text).map((t) => t.word);

describe("words of the prose", () => {
  it("come with offsets of the text, also after an emoji (two UTF-16 units)", () => {
    const text = "😀 Helo wrld";
    const tokens = tokenize(text);
    expect(tokens.map((t) => [t.word, t.from, t.to])).toEqual([
      ["Helo", 3, 7],
      ["wrld", 8, 12],
    ]);
    for (const t of tokens) expect(text.slice(t.from, t.to)).toBe(t.word);
  });

  it("keep an apostrophe | middle dot between letters inside the word (fr, it, ca)", () => {
    expect(words("l'homme dell’anno col·legi")).toEqual(["l'homme", "dell’anno", "col·legi"]);
    // quotes around a word are not part of it
    expect(words("'quoted' word")).toEqual(["quoted", "word"]);
  });

  it("hyphenated: the whole word with its parts", () => {
    const [token] = tokenize("a well-known fact");
    expect(token).toMatchObject({ word: "well-known", from: 2, to: 12 });
    expect(token?.parts.map((p) => [p.word, p.from, p.to])).toEqual([
      ["well", 2, 6],
      ["known", 7, 12],
    ]);
    // single letters are no part of their own
    expect(tokenize("an e-mail")[1]?.parts.map((p) => p.word)).toEqual(["mail"]);
    expect(tokenize("plain")[0]?.parts).toEqual([]);
  });

  it("leave out words no dictionary judges", () => {
    expect(
      words("abc123 NASA camelCase snake_case x I 東京 ひらがな 한국어 COVID-19 kept"),
    ).toEqual(["kept"]);
  });

  it("leave out paths, addresses and dotted names", () => {
    expect(words("see src/main.rs or C:\\dir\\file and main.rs, end.")).toEqual([
      "see",
      "or",
      "and",
      "end",
    ]);
  });
});

describe("what is no prose", () => {
  const cases: [string, string, string[]][] = [
    ["fenced code", "Befor\n\n```\nfenced cdoe\n```\n\nAftr\n", ["Befor", "Aftr"]],
    ["indented code", "Befor\n\n    indented cdoe\n\nAftr\n", ["Befor", "Aftr"]],
    ["inline code", "Use `cdoe here` now", ["Use", "now"]],
    ["display math", "Befor\n\n$$\nx + somthing\n$$\n\nAftr", ["Befor", "Aftr"]],
    ["inline math", "Sum $\\alpha beta$ ends", ["Sum", "ends"]],
    ["HTML block", "<div>\nblock wrods\n</div>\n\nAftr", ["Aftr"]],
    ["inline HTML tag", 'Press <kbd class="kee">Ctrl</kbd> now', ["Press", "Ctrl", "now"]],
    ["HTML comment", "Text <!-- hiden note --> more", ["Text", "more"]],
    ["link destination + title", '[link txt](https://x.org/pth "Titel") ok', ["link", "txt", "ok"]],
    ["image destination", "![alt txt](pics/imgg.png) ok", ["alt", "txt", "ok"]],
    ["reference definition", 'Text\n\n[lbl]: https://x.org "Titel"\n', ["Text"]],
    ["reference label", "[text][lable] ok", ["text", "ok"]],
    ["autolink", "Go <https://exmple.org> now", ["Go", "now"]],
    ["bare URL", "Go https://exmple.org/pth now", ["Go", "now"]],
    ["www URL", "Go www.exmple.org now", ["Go", "now"]],
    ["email", "Mail someone@exmple.org now", ["Mail", "now"]],
    ["front matter (YAML)", "---\ntitel: Somthing\ntaggs: [a]\n---\nBody txt\n", ["Body", "txt"]],
    ["front matter (TOML)", "+++\ntitel = 'Somthing'\n+++\nBody txt\n", ["Body", "txt"]],
    ["wikilink target", "See [[Targt Page]] now", ["See", "now"]],
    ["wikilink alias is prose", "See [[targt|the alias]] now", ["See", "the", "alias", "now"]],
    ["footnote reference", "Text[^nte] more", ["Text", "more"]],
    ["footnote definition label", "Text\n\n[^nte]: The note txt\n", ["Text", "The", "note", "txt"]],
    ["tag", "Filed #projct and #todo-list here", ["Filed", "and", "here"]],
  ];
  for (const [name, text, expected] of cases) {
    it(name, () => expect(words(text)).toEqual(expected));
  }

  it("a heading's words are prose, its marks are not", () => {
    expect(words("# Titel here\n\nBody")).toEqual(["Titel", "here", "Body"]);
  });
});

describe("the language sample", () => {
  it("joins the words, capped", () => {
    expect(sampleOf(tokenize("Hello `code` world"))).toBe("Hello world");
    const long = tokenize("word ".repeat(5000));
    expect(sampleOf(long).length).toBeLessThanOrEqual(SAMPLE_CHARS);
  });
});
