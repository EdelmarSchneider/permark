// Checks on one model output before its suggestions reach the person (spec feature-correction D-1, D-3, D-5): a piece
// whose output leaks thinking, opens with a preamble, breaks the markers | Markdown symbols, changes script (a
// translation), length (dropped | added text) or too many words (a rewrite) is discarded whole. A heading | list item
// never gets a final period added. Thresholds tuned on the eval set (U6).

import { suggest, wordDiff, type Suggestion, type WordDiff } from "./diff";
import { restore, type Piece } from "./segment";

/** Changed-word share past which a piece of at least `RATIO_MIN_WORDS` words is a rewrite. */
export const MAX_CHANGE_RATIO = 0.3;
export const RATIO_MIN_WORDS = 8;
/** Changed words past which a shorter piece is a rewrite. */
export const MAX_CHANGED_WORDS = 3;
/** Output | input length (markers out) must stay in this range... */
export const MIN_LENGTH_RATIO = 0.8;
export const MAX_LENGTH_RATIO = 1.25;
/** ...for inputs at least this long (shorter ones: the changed-word count bounds them). */
export const LENGTH_MIN_CHARS = 20;
/** Share of letters moving between scripts past which the output is another language. */
export const MAX_SCRIPT_SHIFT = 0.2;

/** Openings of a chat answer instead of the text (lowercase; ignored when the original opens the same way). */
const PREAMBLES = [
  "here is",
  "here's",
  "here are",
  "corrected",
  "sure,",
  "sure!",
  "texto corrigido",
  "texto corregido",
  "aquí está",
  "aqui está",
  "voici",
  "hier ist",
  "korrigierter text",
];
const THINKING = "<|channel";
const SYMBOLS = ["*", "_", "[", "]", "~"];
const FINAL_PERIODS = [".", "。"];
const SCRIPTS = [
  /\p{Script=Latin}/u,
  /\p{Script=Han}/u,
  /[\p{Script=Hiragana}\p{Script=Katakana}]/u,
  /\p{Script=Hangul}/u,
  /\p{Script=Cyrillic}/u,
  /\p{Script=Greek}/u,
  /\p{Script=Arabic}/u,
];
const LETTER = /\p{L}/u;
const MARKER = /⟦\d+⟧/g;

export type Discard =
  "thinking" | "preamble" | "markers" | "symbols" | "script" | "length" | "overEdit";

export type Verdict =
  { kind: "ok"; diff: WordDiff; suggestions: Suggestion[] } | { kind: "discard"; reason: Discard };

/** Verdict on the model's `output` for `piece` (cut from `doc`); ok -> its suggestions in document offsets. */
export function judge(doc: string, piece: Piece, output: string, lang?: string): Verdict {
  const discard = (reason: Discard): Verdict => ({ kind: "discard", reason });
  if (output.includes(THINKING)) return discard("thinking");
  if (preamble(piece.text, output)) return discard("preamble");
  const restored = restore(output, piece);
  if (restored === null) return discard("markers");
  if (SYMBOLS.some((s) => count(piece.text, s) !== count(output, s))) return discard("symbols");
  const before = piece.text.replace(MARKER, "");
  const after = output.replace(MARKER, "");
  if (scriptShift(before, after) > MAX_SCRIPT_SHIFT) return discard("script");
  const ratio = after.trim().length / Math.max(1, before.trim().length);
  if (before.length >= LENGTH_MIN_CHARS && (ratio < MIN_LENGTH_RATIO || ratio > MAX_LENGTH_RATIO)) {
    return discard("length");
  }
  const d = wordDiff(piece.original, restored, lang);
  if (overEdit(d)) return discard("overEdit");
  const suggestions = suggest(doc, piece.from, d).flatMap((s) => keepEnding(piece, s));
  return { kind: "ok", diff: d, suggestions };
}

function preamble(input: string, output: string): boolean {
  const opens = (text: string, p: string) => text.trimStart().toLowerCase().startsWith(p);
  return PREAMBLES.some((p) => opens(output, p) && !opens(input, p));
}

function count(text: string, symbol: string): number {
  return text.split(symbol).length - 1;
}

// total variation between the letter shares per script
function scriptShift(a: string, b: string): number {
  const sa = shares(a);
  const sb = shares(b);
  return sa.reduce((sum, share, i) => sum + Math.abs(share - (sb[i] ?? 0)), 0) / 2;
}

// share of letters per entry of `SCRIPTS`, last = any other script
function shares(text: string): number[] {
  const counts = Array.from({ length: SCRIPTS.length + 1 }, () => 0);
  let letters = 0;
  for (const ch of text) {
    if (!LETTER.test(ch)) continue;
    letters += 1;
    const found = SCRIPTS.findIndex((s) => s.test(ch));
    const i = found < 0 ? SCRIPTS.length : found;
    counts[i] = (counts[i] ?? 0) + 1;
  }
  return counts.map((c) => c / Math.max(1, letters));
}

function overEdit(d: WordDiff): boolean {
  const words = d.a.filter((t) => t.word).length;
  let changed = 0;
  for (const op of d.ops) {
    if (op.kind !== "change") continue;
    const removed = d.a.slice(op.a, op.a + op.remove).filter((t) => t.word).length;
    const added = d.b.slice(op.b, op.b + op.add).filter((t) => t.word).length;
    changed += Math.max(removed, added);
  }
  return words >= RATIO_MIN_WORDS
    ? changed / words > MAX_CHANGE_RATIO
    : changed > MAX_CHANGED_WORDS;
}

// heading | list item: a final period the model added is left out (the suggestion dropped when nothing else is left)
function keepEnding(piece: Piece, s: Suggestion): Suggestion[] {
  if ((piece.kind !== "heading" && piece.kind !== "listItem") || s.to !== piece.to) return [s];
  const period = FINAL_PERIODS.find((p) => s.insert.endsWith(p) && !s.original.endsWith(p));
  if (period === undefined) return [s];
  const insert = s.insert.slice(0, -period.length);
  return insert === s.original ? [] : [{ ...s, insert }];
}
