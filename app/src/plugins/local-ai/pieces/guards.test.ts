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

  it("discards dropped | added text by length ratio", () => {
    const doc = "This sentence is complete and has a clear ending.";
    expect(reason(check(only(doc), "This sentence is complete."))).toBe("length");
  });

  it("takes the caller's length range and preambles", () => {
    const doc = "This sentence is complete and has a clear ending.";
    const wide: StructureLimits = { minRatio: 0.3, maxRatio: 3, preambles: [] };
    expect(reason(check(only(doc), "This sentence is complete.", wide))).toBe("ok");
    expect(reason(check(only(doc), `Here is the text: ${doc}`, wide))).toBe("ok");
    expect(reason(check(only(doc), "This.", wide))).toBe("length");
  });
});
