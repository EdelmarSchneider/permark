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
      "Read [teh docs]⟦1⟧ and ⟦2⟧ ⟦3⟧ ⟦4⟧ ⟦5⟧ ⟦6⟧ ⟦7⟧\nline two⟦8⟧three⟦9⟧four",
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
      "<span>teh</span>",
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

describe("quote alerts", () => {
  it("the marker that opens an alert is no part of a piece: the alert's text goes alone", () => {
    const doc = "Intro teh.\n\n> [!NOTE]\n> Useful teh information.\n> More of it.\n";
    const [, piece, ...rest] = pieces(doc);
    expect(rest).toEqual([]);
    expect(piece?.text).toBe("Useful teh information.\n⟦1⟧ More of it.");
    expect(piece?.markers).toEqual([">"]);
    expect(piece?.kind).toBe("quote");
    expect(piece?.from).toBe(doc.indexOf("Useful"));
    expect(piece && restore(piece.text, piece)).toBe(piece?.original);
  });

  it.each(["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION", "note", "Tip", "iMpOrTaNt"])(
    "[!%s]: every kind the reader draws, in any letter case",
    (kind) => {
      const list = pieces(`> [!${kind}]\n> Teh text.\n`);
      expect(texts(list)).toEqual(["Teh text."]);
    },
  );

  it("a title after the marker is the first line of the text; the marker alone holds nothing to send", () => {
    const [titled] = pieces("> [!WARNING] Teh title\n> Body teh.\n");
    expect(titled?.text).toBe("Teh title\n⟦1⟧ Body teh.");
    expect(titled?.original).toBe("Teh title\n> Body teh.");
    expect(pieces("> [!NOTE]\n")).toEqual([]);
    // a paragraph of its own after the marker's line
    expect(texts(pieces("> [!TIP]\n>\n> Teh text.\n"))).toEqual(["Teh text."]);
  });

  it("in a list item and in a quote inside a quote", () => {
    expect(texts(pieces("- > [!NOTE]\n  > Teh item text.\n"))).toEqual(["Teh item text."]);
    expect(texts(pieces("> > [!TIP]\n> > Nested teh tip.\n"))).toEqual(["Nested teh tip."]);
  });

  it("the same text where the reader draws no alert stays text of its piece", () => {
    for (const doc of [
      // not one of the five kinds; no space | two after the `>`; not on the quote's first line
      "> [!INFO]\n> Teh text.\n",
      ">[!NOTE]\n> Teh text.\n",
      ">  [!NOTE]\n> Teh text.\n",
      "> Teh first.\n> [!NOTE]\n> More.\n",
      // outside a quote
      "[!NOTE]\nTeh text.\n",
      "Teh [!NOTE] text.\n",
    ]) {
      const [piece] = pieces(doc);
      expect(piece?.text, doc).toContain("[!");
    }
    // after the marker that opens an alert, the same text is text
    expect(texts(pieces("> [!NOTE]\n> Teh [!NOTE] text.\n"))).toEqual(["Teh [!NOTE] text."]);
  });
});

describe("HTML elements written in a line", () => {
  /** The one piece of `doc`. */
  function only(doc: string): Piece {
    const list = pieces(doc);
    const [piece] = list;
    if (piece === undefined || list.length !== 1) throw new Error(`not one piece in ${doc}`);
    return piece;
  }

  it.each([
    ["Press <kbd>Ctrl</kbd> to save.", "Press ⟦1⟧ to save.", ["<kbd>Ctrl</kbd>"]],
    ["H<sub>2</sub>O is teh water.", "H⟦1⟧O is teh water.", ["<sub>2</sub>"]],
    ['A <span class="x">text</span> here.', "A ⟦1⟧ here.", ['<span class="x">text</span>']],
    // the same name inside: each closing tag ends the nearest one still open
    ["A <span>a <span>b</span> c</span> d.", "A ⟦1⟧ d.", ["<span>a <span>b</span> c</span>"]],
    ["Teh <b>bold <i>both</i></b> end.", "Teh ⟦1⟧ end.", ["<b>bold <i>both</i></b>"]],
    // a `>` in a quoted attribute is no end of the tag
    [
      'See <span title="a > b">text</span> now.',
      "See ⟦1⟧ now.",
      ['<span title="a > b">text</span>'],
    ],
    // an attribute written without quotes takes the `/` before the `>`: an opening tag
    [
      "See <a href=http://x.y/>teh link</a> now.",
      "See ⟦1⟧ now.",
      ["<a href=http://x.y/>teh link</a>"],
    ],
    // tag names in any letter case
    ["Teh <B>bold</b> word.", "Teh ⟦1⟧ word.", ["<B>bold</b>"]],
    // whatever stands inside goes with it: Markdown, code, a link, a formula
    [
      "Teh <span>a *b* `code` [x](u) $y$</span> end.",
      "Teh ⟦1⟧ end.",
      ["<span>a *b* `code` [x](u) $y$</span>"],
    ],
    // its opening tag inside emphasis, its closing tag after it
    ["Teh *a <kbd>b* c</kbd> end.", "Teh *a ⟦1⟧ end.", ["<kbd>b* c</kbd>"]],
    // over a line break; in a quote with the next line's mark
    ["Teh <em>one\ntwo</em> end.", "Teh ⟦1⟧ end.", ["<em>one\ntwo</em>"]],
    ["> Teh <em>one\n> two</em> end.", "Teh ⟦1⟧ end.", ["<em>one\n> two</em>"]],
    ["## Use <kbd>Ctrl</kbd> key", "Use ⟦1⟧ key", ["<kbd>Ctrl</kbd>"]],
    ["- [ ] press <kbd>Ctrl</kbd> twice", "press ⟦1⟧ twice", ["<kbd>Ctrl</kbd>"]],
  ])("%j: the whole element is one marker, its text never sent", (doc, text, markers) => {
    const piece = only(doc);
    expect(piece.text).toBe(text);
    expect(piece.markers).toEqual(markers);
    expect(restore(piece.text, piece)).toBe(piece.original);
  });

  it.each([
    // an opening tag with no closing tag in its block
    ["Teh <b>open text.", "Teh ⟦1⟧open text.", ["<b>"]],
    ["Teh <span>a <b>b</b> c.", "Teh ⟦1⟧a ⟦2⟧ c.", ["<span>", "<b>b</b>"]],
    // a closing tag with no opening tag before it
    ["Teh </i> text.", "Teh ⟦1⟧ text.", ["</i>"]],
    // tags that hold nothing: closed in themselves, or of an element that never has a closing tag
    ["One<br>two <br/> three.", "One⟦1⟧two ⟦2⟧ three.", ["<br>", "<br/>"]],
    [
      'Teh <img src="a.png" alt="x > y"> picture <span/> here.',
      "Teh ⟦1⟧ picture ⟦2⟧ here.",
      ['<img src="a.png" alt="x > y">', "<span/>"],
    ],
    ["One <br> two </br> three.", "One ⟦1⟧ two ⟦2⟧ three.", ["<br>", "</br>"]],
    // closed out of order: the element closed first is the marker, the tag left over stands alone
    ["Teh <b>a <i>b</b> c</i> d.", "Teh ⟦1⟧ c⟦2⟧ d.", ["<b>a <i>b</b>", "</i>"]],
  ])("%j: a tag without its other half stays a marker of its own", (doc, text, markers) => {
    const piece = only(doc);
    expect(piece.text).toBe(text);
    expect(piece.markers).toEqual(markers);
    expect(restore(piece.text, piece)).toBe(piece.original);
  });

  it("the element comes back byte for byte around whatever the answer says", () => {
    const piece = only('Press <kbd class="key">Ctrl</kbd> to save.');
    expect(restore("Drücke ⟦1⟧ zum Speichern.", piece)).toBe(
      'Drücke <kbd class="key">Ctrl</kbd> zum Speichern.',
    );
    expect(restore("Zum Speichern ⟦1⟧ drücken.", piece, "anyOrder")).toBe(
      'Zum Speichern <kbd class="key">Ctrl</kbd> drücken.',
    );
  });

  it("a block that holds nothing but elements is no piece", () => {
    expect(texts(pieces("Intro teh.\n\n<kbd>Ctrl</kbd> + <kbd>S</kbd>\n"))).toEqual(["Intro teh."]);
    expect(pieces("| <b>Name</b> | <i>Note</i> |\n|---|---|\n")).toEqual([]);
  });

  it("in a table cell, with the cell's other text", () => {
    const [cell] = pieces("| Press <kbd>Ctrl</kbd> now | 12 |\n|---|---|\n");
    expect(cell?.text).toBe("Press ⟦1⟧ now");
    expect(cell?.markers).toEqual(["<kbd>Ctrl</kbd>"]);
  });

  it("a scope that ends inside an element takes all of it; a long block is never cut inside one", () => {
    const doc = "Press <kbd>Ctrl</kbd> to save.";
    const inside = doc.indexOf("Ctrl") + 2;
    expect(texts(pieces(doc, { from: 0, to: inside }))).toEqual(["Press ⟦1⟧"]);
    expect(texts(pieces(doc, { from: inside, to: doc.length }))).toEqual(["⟦1⟧ to save."]);
    const element = "<span>First sentence here. Second sentence here. Third one here.</span>";
    const long = `Before the element. ${element} After the element. And more words.`;
    const list = pieces(long, undefined, { chars: 40, cjkChars: 20 });
    expect(list.length).toBeGreaterThan(1);
    expect(list.flatMap((p) => p.markers)).toEqual([element]);
    for (const p of list) expect(restore(p.text, p)).toBe(p.original);
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
    const [html] = pieces("| <b>teh</b> word https://x.y/teh [[Teh page]] |\n|---|\n");
    expect(html?.text).toBe("⟦1⟧ word ⟦2⟧ ⟦3⟧");
    expect(html?.markers[0]).toBe("<b>teh</b>");
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
