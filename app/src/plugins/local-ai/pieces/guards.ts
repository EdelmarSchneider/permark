// Checks any model output for one piece must pass before a plugin reads it as an answer: an output that leaks
// thinking, opens with a preamble, breaks the markers | Markdown symbols | the table row of a cell, brings in Markdown
// the piece does not have (HTML, code, a link's destination, a web address, a line that opens a block) or changes
// length (dropped | added text) beyond the plugin's range is discarded whole. What the answer may change beyond that
// is the plugin's own guard; the count of a text's letters by script (`letterShare`) is here for those.

import { restore, unmarked, type MarkerOrder, type Piece } from "./segment";

/** The length range holds for inputs at least this long (shorter ones: a ratio over a few chars says little). */
export const LENGTH_MIN_CHARS = 20;

const THINKING = "<|channel";
const SYMBOLS = ["*", "_", "[", "]", "~"];
/** What makes more than words of the text beside it: HTML | an autolink (`<`, `>`), code, a picture, the destination
 *  of a link, a web address. A piece shows its own behind markers (`segment.ts`), so one in its text is a plain
 *  char ("a < b"), and an answer may hold as many of each, never more. */
const ACTIVE = ["<", ">", "`", "![", "](", "://"];
/** A line that opens a block by how it starts (spaces before it at most): a heading, a rule | a heading's underline |
 *  a front matter fence (three of the char and more; an underline is one char alone on its line already), a code
 *  fence. Held like `ACTIVE`: no more such lines than the piece has. */
const BLOCK_OPENINGS = [
  /^[ \t]*#/gm,
  /^[ \t]*---/gm,
  /^[ \t]*===/gm,
  /^[ \t]*[-=]{1,2}[ \t]*$/gm,
  /^[ \t]*\+\+\+/gm,
  /^[ \t]*```/gm,
  /^[ \t]*~~~/gm,
];
const LETTER = /\p{L}/u;

export type StructureLimits = {
  /** Output | input length (markers out) must stay in this range. */
  minRatio: number;
  maxRatio: number;
  /** Openings of a chat answer instead of the text (lowercase; ignored when the original opens the same way). */
  preambles: readonly string[];
  /** Where the output's markers may stand (default: in the piece's order); each one exactly once either way. */
  markers?: MarkerOrder;
};

export type StructureDiscard =
  "thinking" | "preamble" | "markers" | "symbols" | "added" | "cell" | "length";

export type Structure =
  /** `restored`: the output with the markers back as original bytes. */
  { kind: "ok"; restored: string } | { kind: "discard"; reason: StructureDiscard };

/** Verdict on the shape of the model's `output` for `piece`: the first check it fails, in the order of
 *  `StructureDiscard`. */
export function structureVerdict(piece: Piece, output: string, limits: StructureLimits): Structure {
  const discard = (reason: StructureDiscard): Structure => ({ kind: "discard", reason });
  if (output.includes(THINKING)) return discard("thinking");
  if (preamble(piece.text, output, limits.preambles)) return discard("preamble");
  const restored = restore(output, piece, limits.markers);
  if (restored === null) return discard("markers");
  if (SYMBOLS.some((s) => count(piece.text, s) !== count(output, s))) return discard("symbols");
  // both with their markers in: what a marker stands for (a quote's `>`, a tag, a code span) is not counted
  if (adds(piece.text, output)) return discard("added");
  if (piece.kind === "cell" && breaksRow(restored.trim())) return discard("cell");
  const before = unmarked(piece.text);
  const after = unmarked(output);
  const ratio = after.trim().length / Math.max(1, before.trim().length);
  if (before.length >= LENGTH_MIN_CHARS && (ratio < limits.minRatio || ratio > limits.maxRatio)) {
    return discard("length");
  }
  return { kind: "ok", restored };
}

/** Share of the letters of `text` that `script` matches (a pattern for one char; 0: no letter in the text). What tells
 *  the script an answer is written in: digits, punctuation and spaces say nothing. */
export function letterShare(text: string, script: RegExp): number {
  let letters = 0;
  let found = 0;
  for (const ch of text) {
    if (!LETTER.test(ch)) continue;
    letters += 1;
    if (script.test(ch)) found += 1;
  }
  return found / Math.max(1, letters);
}

function preamble(input: string, output: string, preambles: readonly string[]): boolean {
  const opens = (text: string, p: string) => text.trimStart().toLowerCase().startsWith(p);
  return preambles.some((p) => opens(output, p) && !opens(input, p));
}

// `text` (an answer without the space around it: a plugin puts the piece's own back) in the place of a table cell
// would change the cells of its row: a line break, a pipe not escaped, or a backslash left open at the end (it would
// escape the pipe that closes the cell). Escapes as the table grammar reads them: `\\|` is a backslash, then a pipe.
function breaksRow(text: string): boolean {
  if (/[\r\n]/.test(text)) return true;
  let escaped = false;
  for (const ch of text) {
    if (ch === "|" && !escaped) return true;
    escaped = !escaped && ch === "\\";
  }
  return escaped;
}

// `output` holds more of an active Markdown token, or more lines that open a block of one kind, than `text`
function adds(text: string, output: string): boolean {
  const lines = (of: string, opening: RegExp): number => of.match(opening)?.length ?? 0;
  return (
    ACTIVE.some((token) => count(output, token) > count(text, token)) ||
    BLOCK_OPENINGS.some((opening) => lines(output, opening) > lines(text, opening))
  );
}

function count(text: string, symbol: string): number {
  return text.split(symbol).length - 1;
}
