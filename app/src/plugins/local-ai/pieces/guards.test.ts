import { describe, expect, it } from "vitest";
import { structureVerdict, type Structure, type StructureLimits } from "./guards";
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
