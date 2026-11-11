import { describe, expect, it } from "vitest";
import { letterShare, structureVerdict, type Structure, type StructureLimits } from "./guards";
import { pieces, type Piece } from "./segment";

const LIMITS: StructureLimits = {
  minRatio: 0.8,
  maxRatio: 1.25,
  preambles: ["here is", "corrected", "texto corrigido"],
};

function only(doc: string): Piece {
  const [piece] = pieces(doc);
  if (piece === undefined) throw new Error(`no piece in ${doc}`);
  return piece;
}

function reason(v: Structure): string {
  return v.kind === "discard" ? v.reason : "ok";
}

function check(piece: Piece, output: string, limits = LIMITS): Structure {
  return structureVerdict(piece, output, limits);
}

describe("structureVerdict", () => {
  it("ok: the output with the markers back as original bytes", () => {
    const piece = only("Use `npm` with *care* now.");
    expect(check(piece, "Use ⟦1⟧ with *care* today.")).toEqual({
      kind: "ok",
      restored: "Use `npm` with *care* today.",
    });
  });

  it("discards a thinking leak and a preamble", () => {
    const doc = "This is fine text.";
    const piece = only(doc);
    expect(reason(check(piece, "<|channel>thought\nThis is fine text."))).toBe("thinking");
    expect(reason(check(piece, "Here is the corrected text: This is fine text."))).toBe("preamble");
    expect(reason(check(piece, "Texto corrigido: This is fine text."))).toBe("preamble");
    const own = "Corrected values are shown below.";
    expect(reason(check(only(own), own))).toBe("ok");
  });

  it("discards tampered markers and changed Markdown symbols", () => {
    const doc = "Use `npm` with *care* now.";
    const piece = only(doc);
    expect(reason(check(piece, "Use npm with *care* now."))).toBe("markers");
    expect(reason(check(piece, "Use ⟦1⟧ with care now."))).toBe("symbols");
    expect(reason(check(piece, "Use ⟦1⟧ with _care_ now."))).toBe("symbols");
  });

  it("markers in the piece's order unless the caller lets them move; each one exactly once either way", () => {
    const piece = only("See `a` and `b` then `c`.");
    const moved = "Then ⟦3⟧, see ⟦1⟧ and ⟦2⟧.";
    expect(reason(check(piece, moved))).toBe("markers");
    expect(reason(check(piece, moved, { ...LIMITS, markers: "inOrder" }))).toBe("markers");
    const free: StructureLimits = { ...LIMITS, markers: "anyOrder" };
    expect(check(piece, moved, free)).toEqual({
      kind: "ok",
      restored: "Then `c`, see `a` and `b`.",
    });
    expect(reason(check(piece, "Then ⟦3⟧, see ⟦1⟧.", free))).toBe("markers");
    expect(reason(check(piece, "Then ⟦3⟧, see ⟦1⟧ and ⟦2⟧ ⟦1⟧.", free))).toBe("markers");
    expect(reason(check(piece, "Then ⟦3⟧, see ⟦1⟧ and ⟦4⟧.", free))).toBe("markers");
    // the other checks hold as ever
    expect(reason(check(piece, "Here is: ⟦3⟧ ⟦1⟧ ⟦2⟧", free))).toBe("preamble");
    const marked = only("Use `npm` with *care* now.");
    expect(reason(check(marked, "Now with care use ⟦1⟧.", free))).toBe("symbols");
  });

  describe("Markdown the piece does not have", () => {
    const PLAIN = "Open the folder and read the guide now.";
    const said = (doc: string, output: string): string => reason(check(only(doc), output));

    it("discards an answer that brings in HTML, an autolink, code or a web address", () => {
      expect(said(PLAIN, "Open the folder <img src=https://x> and read the guide now.")).toBe(
        "added",
      );
      expect(said(PLAIN, "Open the folder <b>and</b> read the guide now.")).toBe("added");
      expect(said(PLAIN, "Open the folder <https://x> and read the guide now.")).toBe("added");
      expect(said(PLAIN, "Open the `folder` and read the guide now.")).toBe("added");
      expect(said(PLAIN, "Open the folder at https://x.y and read it.")).toBe("added");
      // one side of a tag is enough: the other may come from the text around the piece
      expect(said(PLAIN, "Open the folder and read the guide now. <!--")).toBe("added");
      expect(said(PLAIN, "--> Open the folder and read the guide now.")).toBe("added");
    });

    it("discards an answer that gives a link another destination or makes a picture of it", () => {
      const doc = "Read [the guide](docs/guide.md) before you start.";
      const piece = only(doc);
      expect(piece.text).toBe("Read [the guide]⟦1⟧ before you start.");
      expect(
        reason(check(piece, "Read [the guide](javascript:alert(1))⟦1⟧ before you start.")),
      ).toBe("added");
      expect(reason(check(piece, "Read ![the guide]⟦1⟧ before you start."))).toBe("added");
      expect(reason(check(piece, "Read [the guide]⟦1⟧ before you begin."))).toBe("ok");
      // brackets the piece does not have were refused before this rule
      expect(said(PLAIN, "Open [x](javascript:alert(1)) and read the guide now.")).toBe("symbols");
      expect(said(PLAIN, "Open ![alt](u) the folder and read the guide.")).toBe("symbols");
    });

    it("discards an answer with a line that opens a block: a heading, front matter, a rule, a code fence", () => {
      expect(said(PLAIN, "# Open the folder and read the guide.")).toBe("added");
      expect(said(PLAIN, "Open the folder and read\n  ## the guide now.")).toBe("added");
      expect(said(PLAIN, "---\ntitle: Open the folder and read\n---")).toBe("added");
      expect(said(PLAIN, "+++\ntitle = 'Open the folder and read'\n+++")).toBe("added");
      expect(said(PLAIN, "Open the folder and read\n===")).toBe("added");
      // one char alone on a line is a heading's underline already
      expect(said(PLAIN, "Open the folder and read the guide\n=")).toBe("added");
      expect(said(PLAIN, "Open the folder and read the guide\n -- ")).toBe("added");
      expect(said(PLAIN, "Open the folder and read\n- the guide now.")).toBe("ok");
      expect(said(PLAIN, "Open the folder and\n~~~\nread the guide now.")).toBe("symbols");
      const struck = "Open the ~~old~~ folder and read the guide.";
      expect(said(struck, "Open the folder\n~~~~\nand read the guide now.")).toBe("added");
    });

    it("keeps what the piece itself has: as many of each, or fewer", () => {
      const same = (doc: string): string => said(doc, doc);
      expect(same("When a < b the list is sorted, a -> b says so.")).toBe("ok");
      expect(same('Press the ">" key and then the "<" key to move.')).toBe("ok");
      expect(same("C# is fine and so is issue #12 in the list.")).toBe("ok");
      expect(same("Write e.g. http:// before the name of the site.")).toBe("ok");
      expect(same("A lone ` is no code and stays where it is.")).toBe("ok");
      expect(said("When a < b the list is sorted.", "When a is under b it is sorted.")).toBe("ok");
      // one more than the piece has
      expect(said("When a < b the list is sorted.", "When a < b the list is <sorted.")).toBe(
        "added",
      );
      // a line of the piece that starts like a block
      const lines = "The tag\n#todo marks what is left,\n--- so does this.";
      expect(same(lines)).toBe("ok");
      expect(said(lines, "The tag\n#todo marks what is left,\n#too --- so does this.")).toBe(
        "added",
      );
    });

    it("does not count what the piece hides behind markers: a quote's `>`, code, tags, addresses", () => {
      const quote = only("> The weather is nice\n> and the sun shines.\n");
      expect(quote.markers).toEqual([">"]);
      expect(check(quote, "The weather is good\n⟦1⟧ and the sun shines.")).toEqual({
        kind: "ok",
        restored: "The weather is good\n> and the sun shines.",
      });
      // a `>` typed where the marker stood is one the piece does not show
      expect(reason(check(quote, "The weather is good\n⟦1⟧ > and the sun shines."))).toBe("added");
      const marked = only(
        "Run `npm i`, see <https://a.b/c> and <b>this</b> page https://x.y/z now.",
      );
      expect(marked.text).toBe("Run ⟦1⟧, see ⟦2⟧ and ⟦3⟧this⟦4⟧ page ⟦5⟧ now.");
      expect(reason(check(marked, "Run ⟦1⟧, see ⟦2⟧ and ⟦3⟧that⟦4⟧ page ⟦5⟧ now."))).toBe("ok");
    });
  });

  it("discards dropped | added text by length ratio", () => {
    const doc = "This sentence is complete and has a clear ending.";
    expect(reason(check(only(doc), "This sentence is complete."))).toBe("length");
  });

  it("discards an answer that would break a table cell: a pipe, a line break", () => {
    const [head, , cell] = pieces("| Teh head | b |\n|---|---|\n| teh \\| cell | d |\n");
    if (head === undefined || cell === undefined) throw new Error("no cells");
    expect(reason(check(head, "The | head"))).toBe("cell");
    expect(reason(check(head, "The\nhead"))).toBe("cell");
    expect(reason(check(head, "The\r\nhead"))).toBe("cell");
    // a backslash left open at the end would escape the row's own pipe
    expect(reason(check(head, "The head\\"))).toBe("cell");
    // the escaped pipe back in its place; a backslash put before it leaves it bare
    expect(check(cell, "the ⟦1⟧ cell")).toEqual({ kind: "ok", restored: "the \\| cell" });
    expect(reason(check(cell, "the \\⟦1⟧ cell"))).toBe("cell");
    // an escaped pipe is text; space around the answer is not part of it
    expect(reason(check(head, "The \\| head"))).toBe("ok");
    expect(check(head, " The head\n")).toEqual({ kind: "ok", restored: " The head\n" });
  });

  it("a pipe | line break in a paragraph's answer is not this rule's", () => {
    const piece = only("Teh head");
    expect(reason(check(piece, "The | head"))).toBe("ok");
    expect(reason(check(piece, "The\nhead"))).toBe("ok");
    expect(reason(check(piece, "The head\\"))).toBe("ok");
  });

  it("takes the caller's length range and preambles", () => {
    const doc = "This sentence is complete and has a clear ending.";
    const wide: StructureLimits = { minRatio: 0.3, maxRatio: 3, preambles: [] };
    expect(reason(check(only(doc), "This sentence is complete.", wide))).toBe("ok");
    expect(reason(check(only(doc), `Here is the text: ${doc}`, wide))).toBe("ok");
    expect(reason(check(only(doc), "This.", wide))).toBe("length");
  });
});

describe("letterShare", () => {
  it("is the share of a text's letters in a script; digits, marks and spaces are not counted", () => {
    expect(letterShare("abc дe", /\p{Script=Latin}/u)).toBe(0.8);
    expect(letterShare("abc дe", /\p{Script=Cyrillic}/u)).toBe(0.2);
    expect(letterShare("a1 2, 3!", /\p{Script=Latin}/u)).toBe(1);
    expect(letterShare("今日は PerMark", /[\p{Script=Han}\p{Script=Hiragana}]/u)).toBe(0.3);
  });

  it("is 0 for a text without a letter", () => {
    expect(letterShare("12 + 3 = 15", /\p{Script=Latin}/u)).toBe(0);
    expect(letterShare("", /\p{Script=Latin}/u)).toBe(0);
  });
});
