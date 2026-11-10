import { describe, expect, it } from "vitest";
import { CHAR_BUDGET, CJK_CHAR_BUDGET, pieces, restore, type Piece } from "./segment";

const DOC = [
  "---",
  "title: Teh draft",
  "---",
  "# Teh *title* `code`",
  "",
  'Read [teh docs](http://a.b/c "Teh title") and <span>teh</span> $a+b$ [^1] [[Teh page]] https://x.y/teh <http://q.r>',
  "line two  ",
  "three\\",
  "four",
  "",
  "- item won",
  "  more",
  "- [ ] task too",
  "",
  "> quote won",
  "> still",
  "",
  "| teh | b |",
  "|---|---|",
  "| teh | d |",
  "",
  "```js",
  "teh code",
  "```",
  "",
  "    teh indented",
  "",
  "<div>",
  "teh html",
  "</div>",
  "",
  "<!-- teh comment -->",
  "",
  "$$",
  "teh",
  "$$",
  "",
  "[ref]: http://teh",
  "",
].join("\n");

function texts(list: readonly Piece[]): string[] {
  return list.map((p) => p.text);
}

describe("pieces", () => {
  it("sends only prose, with inline non-prose as markers", () => {
    const list = pieces(DOC);
    expect(texts(list)).toEqual([
      "Teh *title* ⟦1⟧",
      "Read [teh docs]⟦1⟧ and ⟦2⟧teh⟦3⟧ ⟦4⟧ ⟦5⟧ ⟦6⟧ ⟦7⟧ ⟦8⟧\nline two⟦9⟧three⟦10⟧four",
      "item won\n  more",
      "task too",
      "quote won\n⟦1⟧ still",
      "teh",
      "b",
      "teh",
      "d",
    ]);
    expect(list.map((p) => p.kind)).toEqual([
      "heading",
      "paragraph",
      "listItem",
      "listItem",
      "quote",
      "cell",
      "cell",
      "cell",
      "cell",
    ]);
    expect(list[1]?.markers).toEqual([
      '(http://a.b/c "Teh title")',
      "<span>",
      "</span>",
      "$a+b$",
      "[^1]",
      "[[Teh page]]",
      "https://x.y/teh",
      "<http://q.r>",
      "  \n",
      "\\\n",
    ]);
  });

  it("keeps the exact document range and restores the original bytes", () => {
    for (const piece of pieces(DOC)) {
      expect(DOC.slice(piece.from, piece.to)).toBe(piece.original);
      expect(restore(piece.text, piece)).toBe(piece.original);
    }
  });

  it("never touches front matter, code, a table's pipes | alignment row, HTML, comments, math or link definitions", () => {
    const covered = pieces(DOC)
      .map((p) => DOC.slice(p.from, p.to))
      .join("\n");
    for (const kept of [
      "title: Teh draft",
      "|",
      "---",
      "teh code",
      "teh indented",
      "teh html",
      "teh comment",
      "$$",
      "http://teh",
    ]) {
      expect(covered).not.toContain(kept);
    }
  });

  it("skips `$$` math only up to its closing `$$`; an unclosed one hides only its own paragraph", () => {
    expect(texts(pieces("$$\na\n\nb\n$$\n\nTeh after."))).toEqual(["Teh after."]);
    expect(texts(pieces("$$\nx = 1\n\nTeh text after.\n\nMore teh."))).toEqual([
      "Teh text after.",
      "More teh.",
    ]);
  });

  it("skips TOML front matter and unclosed YAML front matter like the editor", () => {
    expect(texts(pieces("+++\ntitle = 'teh'\n+++\n\nBody teh."))).toEqual(["Body teh."]);
    expect(pieces("---\ntitle: teh\n\nstill front matter")).toEqual([]);
  });

  it("skips pieces without letters and marks literal marker brackets", () => {
    expect(pieces("123 456\n\n`only code`\n")).toEqual([]);
    const [piece] = pieces("A ⟦1⟧ literal");
    expect(piece?.text).toBe("A ⟦1⟧1⟦2⟧ literal");
    expect(piece && restore(piece.text, piece)).toBe("A ⟦1⟧ literal");
  });

  it("quotation marks are text the model reads and may write another way; only a quote block's `>` is a marker", () => {
    const said = "He said \"teh\", “teh”, «teh», 'teh', ‘teh’ and „teh“.";
    const [plain] = pieces(said);
    expect(plain?.text).toBe(said);
    expect(plain?.markers).toEqual([]);
    const [quoted] = pieces("> “quote won”\n> «still»\n");
    expect(quoted?.text).toBe("“quote won”\n⟦1⟧ «still»");
    expect(quoted?.markers).toEqual([">"]);
  });

  it("clips to the scope, widening a cut mid-word | mid-marker", () => {
    const doc = "First paragraph teh.\n\nSecond won `x` here.\n\nThird.";
    const second = doc.indexOf("Second");
    expect(texts(pieces(doc, { from: second, to: second + 3 }))).toEqual(["Second"]);
    const tick = doc.indexOf("`x`") + 1;
    expect(texts(pieces(doc, { from: doc.indexOf("won"), to: tick }))).toEqual(["won ⟦1⟧"]);
    expect(texts(pieces(doc, { from: 0, to: doc.length }))).toHaveLength(3);
  });

  it("splits long blocks at sentences under the budget, smaller for CJK", () => {
    const en = Array.from({ length: 60 }, (_, i) => `Sentence number ${i} has some words.`).join(
      " ",
    );
    const enPieces = pieces(en);
    expect(enPieces.length).toBeGreaterThan(1);
    for (const p of enPieces) expect(p.original.length).toBeLessThanOrEqual(CHAR_BUDGET);
    expect(enPieces.map((p) => p.original).join(" ")).toBe(en);

    const ja = "今日は天気がとても良いので公園へ散歩に行きました。".repeat(40);
    const jaPieces = pieces(ja);
    expect(jaPieces.length).toBeGreaterThan(1);
    for (const p of jaPieces) expect(p.original.length).toBeLessThanOrEqual(CJK_CHAR_BUDGET);
    expect(jaPieces.map((p) => p.original).join("")).toBe(ja);
    for (const p of [...enPieces, ...jaPieces]) expect(ja + en).toContain(p.original);
  });

  it("takes the caller's budget in place of the default one", () => {
    const en = Array.from({ length: 12 }, (_, i) => `Sentence number ${i} has some words.`).join(
      " ",
    );
    expect(pieces(en)).toHaveLength(1);
    const enPieces = pieces(en, undefined, { chars: 100, cjkChars: 30 });
    expect(enPieces.length).toBeGreaterThan(1);
    for (const p of enPieces) expect(p.original.length).toBeLessThanOrEqual(100);
    expect(enPieces.map((p) => p.original).join(" ")).toBe(en);

    const ja = "今日は天気がとても良いので公園へ散歩に行きました。".repeat(4);
    expect(pieces(ja)).toHaveLength(1);
    const jaPieces = pieces(ja, undefined, { chars: 100, cjkChars: 30 });
    expect(jaPieces).toHaveLength(4);
    expect(jaPieces.map((p) => p.original).join("")).toBe(ja);
  });
});

describe("table cells", () => {
  const TABLE = [
    "| Teh head | Size  |",
    "|:---------|------:|",
    "|  teh  cell   | 12 |",
    "|          | a \\| b teh |",
    "| Use `npm run` teh | [teh docs](http://a.b/c) $x$ |",
    "",
  ].join("\n");

  it("each cell with text is one piece: its text alone, never the padding or a pipe", () => {
    const list = pieces(TABLE);
    expect(texts(list)).toEqual([
      "Teh head",
      "Size",
      "teh  cell",
      "a ⟦1⟧ b teh",
      "Use ⟦1⟧ teh",
      "[teh docs]⟦1⟧ ⟦2⟧",
    ]);
    expect(new Set(list.map((p) => p.kind))).toEqual(new Set(["cell"]));
    // header cells too; the exact range of each
    expect(list.map((p) => [p.from, p.to])).toEqual(
      ["Teh head", "Size", "teh  cell", "a \\| b teh", "Use `npm run` teh"]
        .map((cell) => [TABLE.indexOf(cell), TABLE.indexOf(cell) + cell.length])
        .concat([[TABLE.indexOf("[teh docs]"), TABLE.indexOf("$x$") + 3]]),
    );
    for (const piece of list) {
      expect(TABLE.slice(piece.from, piece.to)).toBe(piece.original);
      expect(piece.original).toBe(piece.original.trim());
      expect(piece.text).not.toContain("|");
      expect(restore(piece.text, piece)).toBe(piece.original);
    }
  });

  it("the alignment row, an empty cell and a cell without letters are never pieces", () => {
    const covered = texts(pieces(TABLE)).join("\n");
    expect(covered).not.toContain("--");
    expect(covered).not.toContain("12");
    expect(pieces("| 1 | 2 |\n|---|---|\n|   | `x` |\n")).toEqual([]);
  });

  it("an escaped pipe is a marker: the model never sees a pipe, the bytes come back", () => {
    const piece = pieces(TABLE)[3];
    if (piece === undefined) throw new Error("no piece");
    expect(piece.markers).toEqual(["\\|"]);
    expect(restore("a ⟦1⟧ b the", piece)).toBe("a \\| b the");
    // in emphasis | a link's text too; outside a table an escaped pipe stays plain text
    const [strong] = pieces("| **teh \\| x** [a \\| b](u) |\n|---|\n");
    expect(strong?.text).toBe("**teh ⟦1⟧ x** [a ⟦2⟧ b]⟦3⟧");
    expect(texts(pieces("Teh \\| pipe"))).toEqual(["Teh \\| pipe"]);
  });

  it("inline non-prose in a cell is a marker, as in a paragraph", () => {
    const list = pieces(TABLE);
    expect(list[4]?.markers).toEqual(["`npm run`"]);
    expect(list[5]?.markers).toEqual(["(http://a.b/c)", "$x$"]);
    const [html] = pieces("| <b>teh</b> https://x.y/teh [[Teh page]] |\n|---|\n");
    expect(html?.text).toBe("⟦1⟧teh⟦2⟧ ⟦3⟧ ⟦4⟧");
  });

  it("a table in a quote | a list item, and one written without outer pipes", () => {
    const quote = "> | teh quote | b |\n> |---|---|\n> | c | teh |\n";
    expect(texts(pieces(quote))).toEqual(["teh quote", "b", "c", "teh"]);
    expect(new Set(pieces(quote).map((p) => p.kind))).toEqual(new Set(["cell"]));
    const item = "- item\n\n  | teh list | x |\n  |---|---|\n  | y | z |\n";
    expect(texts(pieces(item))).toEqual(["item", "teh list", "x", "y", "z"]);
    expect(pieces(item).map((p) => p.kind)).toEqual(["listItem", "cell", "cell", "cell", "cell"]);
    expect(texts(pieces("teh a | b\n--|--\nteh one | two\n"))).toEqual([
      "teh a",
      "b",
      "teh one",
      "two",
    ]);
  });

  it("a table the grammar does not read as one is left alone, never sent as a paragraph", () => {
    const rows = ["| Teh head | b |", "|---|---|", "| teh | d |", "", "Teh after.", ""];
    // CRLF line ends (the editor's text has LF; a text that comes as the file has it)
    expect(texts(pieces(rows.join("\r\n")))).toEqual(["Teh after."]);
    // spaces after the alignment row
    expect(texts(pieces(rows.join("\n").replace("|---|---|", "|---|---|  ")))).toEqual([
      "Teh after.",
    ]);
    // right under a paragraph's line the grammar reads it: cells as ever
    expect(texts(pieces(`Teh intro\n${rows.join("\n")}`))).toEqual([
      "Teh intro",
      "Teh head",
      "b",
      "teh",
      "d",
      "Teh after.",
    ]);
    // dashes and a pipe among words are no alignment row
    expect(texts(pieces("Teh a - b\nc -- d | e"))).toEqual(["Teh a - b\nc -- d | e"]);
  });

  it("clips to the scope: the cells it touches, each cut as a paragraph is", () => {
    const doc = "| Teh head | Size here |\n|---|---|\n| teh cell | more words |\n";
    const cell = doc.indexOf("teh cell");
    const more = doc.indexOf("more");
    // inside one cell; a cut mid-word widens to the word
    expect(texts(pieces(doc, { from: cell, to: cell + 8 }))).toEqual(["teh cell"]);
    expect(texts(pieces(doc, { from: cell + 5, to: cell + 6 }))).toEqual(["cell"]);
    // across two cells: the padding and the pipe between them belong to neither
    const across = pieces(doc, { from: cell + 4, to: more + 4 });
    expect(texts(across)).toEqual(["cell", "more"]);
    expect(across.map((p) => [p.from, p.to])).toEqual([
      [cell + 4, cell + 8],
      [more, more + 4],
    ]);
    // across rows: the alignment row between them is never a piece
    expect(texts(pieces(doc, { from: doc.indexOf("Size"), to: cell + 3 }))).toEqual([
      "Size here",
      "teh",
    ]);
    // only pipes, padding or the alignment row
    const row = doc.indexOf("|---");
    expect(pieces(doc, { from: row, to: row + 9 })).toEqual([]);
    expect(pieces(doc, { from: cell + 8, to: more })).toEqual([]);
  });
});

describe("restore", () => {
  const [piece] = pieces("See `a` and `b` then `c`.");

  it("round-trips the markers", () => {
    expect(piece?.text).toBe("See ⟦1⟧ and ⟦2⟧ then ⟦3⟧.");
    expect(piece && restore("See ⟦1⟧, and ⟦2⟧ then ⟦3⟧.", piece)).toBe(
      "See `a`, and `b` then `c`.",
    );
  });

  it("refuses a missing, reordered, repeated or broken marker", () => {
    if (piece === undefined) throw new Error("no piece");
    for (const bad of [
      "See ⟦1⟧ and then ⟦3⟧.",
      "See ⟦2⟧ and ⟦1⟧ then ⟦3⟧.",
      "See ⟦1⟧ and ⟦2⟧ then ⟦3⟧ ⟦3⟧.",
      "See ⟦1⟧ and ⟦2⟧ then ⟦3⟧ ⟦4⟧.",
      "See ⟦1⟧ and ⟦2⟧ then ⟦3.",
    ]) {
      expect(restore(bad, piece), bad).toBeNull();
    }
  });

  it("any order when the caller lets the markers move: each one still exactly once", () => {
    if (piece === undefined) throw new Error("no piece");
    expect(restore("Then ⟦3⟧, see ⟦2⟧ and ⟦1⟧.", piece, "anyOrder")).toBe(
      "Then `c`, see `b` and `a`.",
    );
    expect(restore("See ⟦1⟧ and ⟦2⟧ then ⟦3⟧.", piece, "anyOrder")).toBe(piece.original);
    for (const bad of [
      "Then ⟦3⟧, see ⟦1⟧.",
      "Then ⟦3⟧, see ⟦2⟧ and ⟦1⟧ ⟦2⟧.",
      "Then ⟦3⟧, see ⟦1⟧ and ⟦1⟧.",
      "Then ⟦3⟧, see ⟦2⟧ and ⟦4⟧.",
      "Then ⟦3⟧, see ⟦2⟧ and ⟦1⟧ ⟦0⟧.",
      "Then ⟦3⟧, see ⟦2⟧ and ⟦1.",
    ]) {
      expect(restore(bad, piece, "anyOrder"), bad).toBeNull();
    }
  });
});
