// The words a spelling check looks at: prose only, with the editors' UTF-16 offsets.
import { describe, expect, it } from "vitest";
import {
  sampleOf,
  SAMPLE_CHARS,
  SLICE_CHARS,
  tokenize,
  tokenizeInSteps,
  tokenSteps,
} from "./tokenize";

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

  it("read Cyrillic words, with the Ukrainian apostrophe in every form inside the word", () => {
    expect(words("Пекарня на углу продаёт хлеб, ёлка и съезд.")).toEqual([
      "Пекарня",
      "на",
      "углу",
      "продаёт",
      "хлеб",
      "ёлка",
      "съезд",
    ]);
    // U+02BC (the standard one), the typewriter and the typographic apostrophe
    expect(words("мʼясо м'ясо м’ясо ґанок їжак Європа")).toEqual([
      "мʼясо",
      "м'ясо",
      "м’ясо",
      "ґанок",
      "їжак",
      "Європа",
    ]);
    // all capitals and single letters stay out, as in Latin script
    expect(words("СССР я і книга")).toEqual(["книга"]);
  });

  it("read Polish and Turkish letters, dotted and dotless i included", () => {
    expect(words("Źródło żółć Łódź książka")).toEqual(["Źródło", "żółć", "Łódź", "książka"]);
    expect(words("İstanbul ılık ışık Iğdır")).toEqual(["İstanbul", "ılık", "ışık", "Iğdır"]);
  });

  it("read Vietnamese syllables, composed or with combining marks, at their own offsets", () => {
    expect(words("Tiệm bánh ở góc phố bán bánh mì ngon nhất")).toEqual([
      "Tiệm",
      "bánh",
      "góc",
      "phố",
      "bán",
      "bánh",
      "mì",
      "ngon",
      "nhất",
    ]);
    const text = "nha\u0300 ngu\u031Bo\u031B\u0300i";
    const tokens = tokenize(text);
    expect(tokens.map((t) => [t.word, t.from, t.to])).toEqual([
      ["nha\u0300", 0, 4],
      ["ngu\u031Bo\u031B\u0300i", 5, 13],
    ]);
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

  it("a document that opens with a thematic break: no front matter, its prose is read", () => {
    expect(words("---\n\nSome prose wrods\n\n---\nMore txt\n")).toEqual([
      "Some",
      "prose",
      "wrods",
      "More",
      "txt",
    ]);
    expect(words("---\nJust one line of prose\n---\nAftr\n")).toEqual([
      "Just",
      "one",
      "line",
      "of",
      "prose",
      "Aftr",
    ]);
    // a comment line or a TOML table opens real front matter too
    expect(words("---\n# a commnt\ntitel: x\n---\nBody\n")).toEqual(["Body"]);
    expect(words("+++\n[tabel]\nkey = 1\n+++\nBody\n")).toEqual(["Body"]);
  });
});

describe("Windows line breaks (CRLF)", () => {
  it("words keep the offsets of the text", () => {
    const text = "Helo\r\nwrld here\r\n\r\nend";
    const tokens = tokenize(text);
    expect(tokens.map((t) => [t.word, t.from, t.to])).toEqual([
      ["Helo", 0, 4],
      ["wrld", 6, 10],
      ["here", 11, 15],
      ["end", 19, 22],
    ]);
    for (const t of tokens) expect(text.slice(t.from, t.to)).toBe(t.word);
  });

  it("code, math and front matter stay masked", () => {
    expect(words("---\r\ntitel: Somthing\r\n---\r\nBody txt\r\n")).toEqual(["Body", "txt"]);
    expect(words("Befor\r\n\r\n```\r\nfenced cdoe\r\n```\r\n\r\nAftr\r\n")).toEqual([
      "Befor",
      "Aftr",
    ]);
    expect(words("Befor\r\n\r\n$$\r\nx + somthing\r\n$$\r\n\r\nAftr")).toEqual(["Befor", "Aftr"]);
    expect(tokenize("a well-known\r\nfact")[0]?.parts.map((p) => p.word)).toEqual([
      "well",
      "known",
    ]);
  });
});

/** About `size` characters of Markdown: prose with links, code, math, wikilinks, tags and lists (CRLF every 3rd). */
function largeDocument(size: number): string {
  const block = (i: number): string =>
    [
      `## Section ${toWord(i)} heading`,
      "",
      `The quick brown fox jumps over the lazy dog, with a [link text](https://example.org/p${i} "Title") and`,
      "`inline code` plus *emphasis* and a #tag and [[Wiki Page|an alias]] and $x + y$ and a mispeled word here.",
      "",
      "- a list item with words",
      `- another item, the ${toWord(i)} one`,
      "",
      "```js",
      'const code = "not prose";',
      "```",
      "",
      "",
    ].join(i % 3 === 0 ? "\r\n" : "\n");
  let out = "";
  for (let i = 0; out.length < size; i += 1) out += block(i);
  return out;
}

/** `n` as letters only (digits leave a word out). */
function toWord(n: number): string {
  let out = "";
  let rest = n;
  do {
    out += "abcdefghijklmnopqrstuvwxyz"[rest % 26] ?? "";
    rest = Math.floor(rest / 26);
  } while (rest > 0);
  return out;
}

describe("a large document", () => {
  const text = largeDocument(2_000_000);

  it("is read in bounded steps, with the same words as in one go", () => {
    const steps = tokenSteps(text);
    let last = 0;
    let longest = 0;
    let count = 0;
    let step = steps.next();
    while (step.done !== true) {
      // the parse phase, then the word phase: each counts the offset it reached from 0
      if (step.value > last) longest = Math.max(longest, step.value - last);
      last = step.value;
      count += 1;
      step = steps.next();
    }
    expect(count).toBeGreaterThan(text.length / SLICE_CHARS);
    expect(longest).toBeLessThanOrEqual(2 * SLICE_CHARS);
    const sliced = step.value;
    expect(sliced.length).toBeGreaterThan(100_000);
    expect(sliced).toEqual(tokenize(text, Number.POSITIVE_INFINITY));
    // a cut that lands inside a wikilink or inline math (a slice ends at a space when no line break is near)
    const oneLine = "See [[Targt Page]] and $a b$ hello ".repeat(2000);
    expect(tokenize(oneLine, 64)).toEqual(tokenize(oneLine, Number.POSITIVE_INFINITY));
  });

  it("in runs that pause between them; dropped once no longer wanted", async () => {
    let pauses = 0;
    const pause = (): Promise<void> => {
      pauses += 1;
      return Promise.resolve();
    };
    const found = await tokenizeInSteps(text, () => false, pause);
    expect(pauses).toBeGreaterThan(1);
    expect(found?.length).toBe(tokenize(text).length);
    expect(await tokenizeInSteps(text, () => true, pause)).toBeNull();
    // a small text: read in one run
    pauses = 0;
    expect(await tokenizeInSteps("Hello wrold", () => true, pause)).toHaveLength(2);
    expect(pauses).toBe(0);
  });
});

describe("the language sample", () => {
  it("joins the words, capped", () => {
    expect(sampleOf(tokenize("Hello `code` world"))).toBe("Hello world");
    const long = tokenize("word ".repeat(5000));
    expect(sampleOf(long).length).toBeLessThanOrEqual(SAMPLE_CHARS);
  });
});
