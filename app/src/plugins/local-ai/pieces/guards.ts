// Checks any model output for one piece must pass before a plugin reads it as an answer: an output that leaks
// thinking, opens with a preamble, breaks the markers | Markdown symbols or changes length (dropped | added text)
// beyond the plugin's range is discarded whole. What the answer may change beyond that is the plugin's own guard.

import { restore, unmarked, type Piece } from "./segment";

/** The length range holds for inputs at least this long (shorter ones: a ratio over a few chars says little). */
export const LENGTH_MIN_CHARS = 20;

const THINKING = "<|channel";
const SYMBOLS = ["*", "_", "[", "]", "~"];

export type StructureLimits = {
  /** Output | input length (markers out) must stay in this range. */
  minRatio: number;
  maxRatio: number;
  /** Openings of a chat answer instead of the text (lowercase; ignored when the original opens the same way). */
  preambles: readonly string[];
};

export type StructureDiscard = "thinking" | "preamble" | "markers" | "symbols" | "length";

export type Structure =
  /** `restored`: the output with the markers back as original bytes. */
  { kind: "ok"; restored: string } | { kind: "discard"; reason: StructureDiscard };

/** Verdict on the shape of the model's `output` for `piece`: the first check it fails, in the order of
 *  `StructureDiscard`. */
export function structureVerdict(piece: Piece, output: string, limits: StructureLimits): Structure {
  const discard = (reason: StructureDiscard): Structure => ({ kind: "discard", reason });
  if (output.includes(THINKING)) return discard("thinking");
  if (preamble(piece.text, output, limits.preambles)) return discard("preamble");
  const restored = restore(output, piece);
  if (restored === null) return discard("markers");
  if (SYMBOLS.some((s) => count(piece.text, s) !== count(output, s))) return discard("symbols");
  const before = unmarked(piece.text);
  const after = unmarked(output);
  const ratio = after.trim().length / Math.max(1, before.trim().length);
  if (before.length >= LENGTH_MIN_CHARS && (ratio < limits.minRatio || ratio > limits.maxRatio)) {
    return discard("length");
  }
  return { kind: "ok", restored };
}

function preamble(input: string, output: string, preambles: readonly string[]): boolean {
  const opens = (text: string, p: string) => text.trimStart().toLowerCase().startsWith(p);
  return preambles.some((p) => opens(output, p) && !opens(input, p));
}

function count(text: string, symbol: string): number {
  return text.split(symbol).length - 1;
}
