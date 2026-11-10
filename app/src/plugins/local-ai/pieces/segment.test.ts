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
    ]);
    expect(list.map((p) => p.kind)).toEqual([
      "heading",
      "paragraph",
      "listItem",
      "listItem",
      "quote",
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

  it("never touches front matter, code, tables, HTML, comments, math or link definitions", () => {
    const covered = pieces(DOC)
      .map((p) => DOC.slice(p.from, p.to))
      .join("\n");
    for (const kept of [
      "title: Teh draft",
      "| teh |",
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
});
