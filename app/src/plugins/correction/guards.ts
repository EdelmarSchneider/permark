// Checks on one model output before its suggestions reach the person: a piece whose output fails the structure check
// of `local-ai` (leaked thinking, a preamble, broken markers | Markdown symbols, length: dropped | added text),
// changes script (a translation) or too many words (a rewrite) is discarded whole. A heading | list item never gets a
// final period added. Thresholds tuned on the eval set (`eval/`).

import {
  structureVerdict,
  type StructureDiscard,
  type StructureLimits,
} from "../local-ai/pieces/guards";
import { unmarked, type Piece } from "../local-ai/pieces/segment";
import type { Suggestion } from "../local-ai/pieces/suggestion";
import { suggest, wordDiff, type WordDiff } from "./diff";

/** Changed-word share past which a piece of at least `RATIO_MIN_WORDS` words is a rewrite. */
export const MAX_CHANGE_RATIO = 0.3;
export const RATIO_MIN_WORDS = 8;
/** Changed words past which a shorter piece is a rewrite. */
export const MAX_CHANGED_WORDS = 3;
/**
 * Pieces of at most this many words (a heading, a label): more than half of the words changed into text that does not
 * look alike = a rewrite | translation ("Una casa blanca" -> "A white house"); a typo fix stays ("# Introdution").
 */
export const SHORT_WORDS = 3;
/** Character similarity (1 - edit distance / longer length) under which a changed short word is a different word. */
export const MIN_WORD_SIMILARITY = 0.5;
/** Output | input length (markers out) must stay in this range (inputs too short for it: the changed-word count
 *  bounds them). */
export const MIN_LENGTH_RATIO = 0.8;
export const MAX_LENGTH_RATIO = 1.25;
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
/** What the structure check holds a corrected piece to. */
const STRUCTURE: StructureLimits = {
  minRatio: MIN_LENGTH_RATIO,
  maxRatio: MAX_LENGTH_RATIO,
  preambles: PREAMBLES,
};
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

export type Discard = StructureDiscard | "script" | "overEdit";

export type Verdict =
  { kind: "ok"; diff: WordDiff; suggestions: Suggestion[] } | { kind: "discard"; reason: Discard };

/** Verdict on the model's `output` for `piece` (cut from `doc`); ok -> its suggestions in document offsets. */
export function judge(doc: string, piece: Piece, output: string, lang?: string): Verdict {
  const discard = (reason: Discard): Verdict => ({ kind: "discard", reason });
  const structure = structureVerdict(piece, output, STRUCTURE);
  // length is the structure check's last: a changed script is told before it (a translation changes the length too)
  if (structure.kind === "discard" && structure.reason !== "length") return structure;
  if (scriptShift(unmarked(piece.text), unmarked(output)) > MAX_SCRIPT_SHIFT)
    return discard("script");
  if (structure.kind === "discard") return structure;
  const d = wordDiff(piece.original, structure.restored, lang);
  if (overEdit(d)) return discard("overEdit");
  const suggestions = suggest(doc, piece.from, d).flatMap((s) => keepEnding(piece, s));
  return { kind: "ok", diff: d, suggestions };
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
  let unlike = 0;
  for (const op of d.ops) {
    if (op.kind !== "change") continue;
    const removed = d.a.slice(op.a, op.a + op.remove);
    const added = d.b.slice(op.b, op.b + op.add);
    const count = Math.max(
      removed.filter((t) => t.word).length,
      added.filter((t) => t.word).length,
    );
    changed += count;
    const text = (tokens: typeof removed) => tokens.map((t) => t.text).join("");
    if (similarity(text(removed), text(added)) < MIN_WORD_SIMILARITY) unlike += count;
  }
  if (words >= RATIO_MIN_WORDS) return changed / words > MAX_CHANGE_RATIO;
  if (words <= SHORT_WORDS) return unlike > words / 2;
  return changed > MAX_CHANGED_WORDS;
}

// 1 - Levenshtein distance / longer length, over code points (1 = same, 0 = nothing alike)
function similarity(a: string, b: string): number {
  const x = [...a.normalize("NFC").toLowerCase()];
  const y = [...b.normalize("NFC").toLowerCase()];
  const longer = Math.max(x.length, y.length);
  if (longer === 0) return 1;
  let row = Array.from({ length: y.length + 1 }, (_, j) => j);
  for (let i = 1; i <= x.length; i += 1) {
    const next = [i];
    for (let j = 1; j <= y.length; j += 1) {
      const cost = x[i - 1] === y[j - 1] ? 0 : 1;
      next.push(Math.min((row[j] ?? 0) + 1, (next[j - 1] ?? 0) + 1, (row[j - 1] ?? 0) + cost));
    }
    row = next;
  }
  return 1 - (row[y.length] ?? longer) / longer;
}

// heading | list item: a final period the model added is left out (the suggestion dropped when nothing else is left)
function keepEnding(piece: Piece, s: Suggestion): Suggestion[] {
  if ((piece.kind !== "heading" && piece.kind !== "listItem") || s.to !== piece.to) return [s];
  const period = FINAL_PERIODS.find((p) => s.insert.endsWith(p) && !s.original.endsWith(p));
  if (period === undefined) return [s];
  const insert = s.insert.slice(0, -period.length);
  return insert === s.original ? [] : [{ ...s, insert }];
}
