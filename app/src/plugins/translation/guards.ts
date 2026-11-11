// Checks on one model output before its translation reaches the person. The first check that fails decides, in this
// order:
// 1. structure (`local-ai`): leaked thinking, an opening that announces a translation, markers (each once; anywhere:
//    a translation moves the words around them), Markdown symbol counts, Markdown the piece does not have (HTML,
//    code, a link's destination, a web address, a line that opens a block), the row of a table cell -> discarded;
// 2. no letter in the answer -> discarded;
// 3. lines: a marker that holds the lines of its block where it no longer does (a quote's `>` not opening a line, a
//    hard break at an end of the answer) -> discarded;
// 4. quotation marks around the whole answer that the piece does not have are taken off (the model quoting its
//    answer): never a discard, and before 5 (a quoted copy of the piece is the piece);
// 5. the same text as the piece -> nothing to suggest (`same`); before the script check: a name reads the same in
//    every language. Nearly the same text (`NEAR_SAME`) too, when the run reads the source off the text and the
//    piece is not a short one (`NEAR_SAME_MIN_CHARS`): the piece was in the target already, and a model rewords such
//    text a little instead of repeating it (a rule in the prompt did not stop it). Not when the person named the
//    source: a translation between close variants of a language (pt-PT into pt-BR) is as close, and is what was
//    asked for;
// 6. a line without text inside the answer (a note | a second answer after the translation; it would end the
//    paragraph) -> discarded;
// 7. script: under half of the letters in the target's script, when the runtime knows it -> discarded. Words the
//    answer copied from the piece are left out of the count (`WORD`): a translation keeps product and file names as
//    they are written, and in a script that says much in few letters they outnumber the rest ("Node.jsガイド");
// 8. length (the structure check's own, its range by how dense the scripts of the two texts are) -> discarded; last,
//    so that an answer that is something else than a translation is told as what it is.
// What passes is ONE suggestion for the whole piece; a piece written on one line gets its answer on one line (after
// the checks: they read the answer as the model wrote it). Not told here: a wrong language in the right script
// (Spanish for Portuguese), and, with the source named, the piece left in its language with nothing but words of its
// own (a word dropped, a comma added: every letter is a copy). Told wrongly as `same`, with the source read off the
// text: a translation of a longer piece that changes a tenth of its chars at most (a long line of names with one
// small word between them; a paragraph between close variants of a language). The length limits are first values.

import {
  letterShare,
  similarity,
  structureVerdict,
  type StructureDiscard,
} from "../local-ai/pieces/guards";
import type { PieceVerdict } from "../local-ai/pieces/review.svelte";
import { mostlyCjk, restore, unmarked, type Piece } from "../local-ai/pieces/segment";
import { around } from "../local-ai/pieces/suggestion";
import { scriptOf } from "./languages";

/** Languages of a run, as tags: translated into `target`, from `source` (`null`: read off each piece). */
export type Pair = { target: string; source: string | null };

export type RatioRange = { min: number; max: number };

/** Output | input length (markers out) a translation stays in, for pieces long enough for a ratio to say something
 *  (`LENGTH_MIN_CHARS` of `local-ai`), by how dense the scripts of the two texts are (dense: mostly Han | Kana |
 *  Hangul). */
export const LENGTH_RATIO: Readonly<Record<"sameDensity" | "intoDense" | "fromDense", RatioRange>> =
  {
    /** Alphabetic text into alphabetic text, dense into dense. */
    sameDensity: { min: 0.5, max: 2.0 },
    /** Alphabetic text into a dense script: a few chars say a word. */
    intoDense: { min: 0.2, max: 1.2 },
    /** A dense script into alphabetic text. */
    fromDense: { min: 1.0, max: 5.0 },
  };
/** Share of an answer's letters that must be in the target's script (the words it copied from the piece left out). */
export const MIN_SCRIPT_SHARE = 0.5;
/** How alike an answer and its piece are, at least (`similarity` of `local-ai`), for the answer to be the piece
 *  itself, a little reworded. From the answers of a real model: for text that was in the target already they were
 *  0.93 alike and more, the closest translation (Spanish into Portuguese) 0.70. */
export const NEAR_SAME = 0.9;
/** Shortest piece (chars; `dense`: mostly Han | Kana | Hangul, where a few chars say a word) the near-same check
 *  reads. In a shorter one a real translation can leave nine chars in ten as they are (names with one small word
 *  between them: "Chrome, Firefox and Safari"; a word spelled almost alike in both languages: "Dokumentation"), so
 *  there only the same text counts. */
export const NEAR_SAME_MIN_CHARS: Readonly<Record<"alphabetic" | "dense", number>> = {
  alphabetic: 60,
  dense: 20,
};
/** A word, as far as a copied one is told: a run of Latin letters | digits with `.` `_` `-` only between them (how a
 *  name, a file name, an identifier is written: "Node.js", "max_retries", "no-cache"; the period that ends a
 *  sentence is no part of the word before it), or a run of letters of the other scripts. Latin letters and others
 *  are never one word: scripts written without spaces put a name right beside their own words ("Node.jsガイド"). */
const WORD =
  /[\p{Script=Latin}\p{N}]+(?:[._-]+[\p{Script=Latin}\p{N}]+)*|[^\P{L}\p{Script=Latin}]+/gu;

/** Openings of a chat answer instead of the translation (lowercase). Only ones that announce a translation: "here
 *  is" alone opens many a translated sentence. */
const PREAMBLES = [
  "here is the transl",
  "here's the transl",
  "here is your transl",
  "here's your transl",
  "sure, here",
  "sure! here",
  "translation:",
  "translated text:",
  "tradução:",
  "traducción:",
  "traduction:",
  "traduction :",
  "übersetzung:",
  "traduzione:",
  "翻訳:",
  "翻訳：",
  "翻译:",
  "翻译：",
  "번역:",
];
/** Unicode scripts behind a locale's script code that stands for several. */
const SCRIPTS_OF: Readonly<Record<string, readonly string[]>> = {
  Jpan: ["Han", "Hiragana", "Katakana"],
  Kore: ["Hangul", "Han"],
  Hans: ["Han"],
  Hant: ["Han"],
};
/** Opening and closing mark of the ways an answer gets quoted. */
const QUOTES: readonly (readonly [string, string])[] = [
  ['"', '"'],
  ["'", "'"],
  ["“", "”"],
  ["‘", "’"],
  ["„", "“"],
  ["„", "”"],
  ["«", "»"],
  ["»", "«"],
  ["‹", "›"],
  ["「", "」"],
  ["『", "』"],
];
/** A quote block's `>` as the pieces hide it on each line after the first (several: nested with no space between). */
const QUOTE_MARK = /^>+$/;
const LETTER = /\p{L}/u;
/** A line that holds no text: at most spaces, a quote's `>`, the backslash of a hard break. */
const BARE_LINE = /^[\s>\\]*$/;
/** A char of a script written without spaces between words: Han, Kana, their punctuation, fullwidth forms. */
const UNSPACED = String.raw`[\p{Script_Extensions=Han}\p{Script_Extensions=Hiragana}\p{Script_Extensions=Katakana}\uFF01-\uFF60]`;
const ENDS_UNSPACED = new RegExp(`${UNSPACED}$`, "u");
const STARTS_UNSPACED = new RegExp(`^${UNSPACED}`, "u");
/** First of the private-use chars a stand-in is picked from. */
const PRIVATE_USE = 0xe000;

export type Discard = StructureDiscard | "empty" | "lines" | "blankLine" | "script";

/** Verdict on the model's `output` for `piece` (cut from `doc`) in a run of `pair`: ok -> the one suggestion that
 *  puts the translation in the piece's place, in document offsets. */
export function judge(doc: string, piece: Piece, output: string, pair: Pair): PieceVerdict {
  const discard = (reason: Discard): PieceVerdict => ({ kind: "discard", reason });
  // the editor's text is LF; space around an answer is not part of it
  const said = output.replace(/\r\n?/g, "\n").trim();
  const structure = structureVerdict(piece, said, {
    markers: "anyOrder",
    ...lengthRange(piece.text, said),
    preambles: opensLikeAnAnswer(piece.text) ? [] : PREAMBLES,
  });
  // the length is the structure check's last: what else the answer is (no text, the piece again, a note after the
  // translation, another script) is told before it; its markers passed, so they go back whatever the length
  if (structure.kind === "discard" && structure.reason !== "length") return structure;
  if (!LETTER.test(unmarked(said))) return discard("empty");
  const restored =
    structure.kind === "ok" ? structure.restored : (restore(said, piece, "anyOrder") ?? said);
  const lines = restored.split("\n").map((line) => BARE_LINE.test(line));
  // the answer came without space around it: an end line without text is a marker's (a hard break with no words on
  // one side of it)
  if (lines[0] === true || lines.at(-1) === true || quoteMarkAdrift(piece, said))
    return discard("lines");
  const bare = unquoted(restored);
  const answer = bare !== null && unquoted(piece.original) === null ? bare : restored;
  if (isThePiece(answer, piece.original, pair.source === null)) return { kind: "same" };
  if (lines.includes(true)) return discard("blankLine");
  if (offScript(unmarked(said), unmarked(piece.text), pair.target)) return discard("script");
  if (structure.kind === "discard") return structure;
  const insert = piece.original.includes("\n") ? answer : oneLine(answer);
  const { from, to, original, kind: within } = piece;
  const excerpt = around(doc, from, to, within);
  return { kind: "ok", suggestions: [{ from, to, original, insert, within, ...excerpt }] };
}

// the length range for a piece's text and its answer (markers hold no letter: they count for neither script)
function lengthRange(text: string, output: string): { minRatio: number; maxRatio: number } {
  const dense = mostlyCjk(output);
  const kind = mostlyCjk(text) === dense ? "sameDensity" : dense ? "intoDense" : "fromDense";
  return { minRatio: LENGTH_RATIO[kind].min, maxRatio: LENGTH_RATIO[kind].max };
}

// the piece itself opens like an answer's opening, in whatever language ("Translation: …"): its translation opens
// the same way in another one, so none of them tells an answer apart
function opensLikeAnAnswer(text: string): boolean {
  const start = text.trimStart().toLowerCase();
  return PREAMBLES.some((p) => start.startsWith(p));
}

// A quote's `>` is a marker on each line of the piece after the first, and reads as the quote's mark only where it
// opens a line (spaces | other `>` before it at most). Markers may stand anywhere in an answer: true when one of
// these stands elsewhere (inside a line, on the first line, its line break gone). A hard break needs no such check:
// its marker holds the line break (and the next line's `>` in a quote). Seen by putting a stand-in back for these
// markers: a char neither the answer nor a marker holds.
function quoteMarkAdrift(piece: Piece, output: string): boolean {
  if (!piece.markers.some((m) => QUOTE_MARK.test(m))) return false;
  const held = output + piece.markers.join("");
  let code = PRIVATE_USE;
  while (held.includes(String.fromCharCode(code))) code += 1;
  const mark = String.fromCharCode(code);
  const markers = piece.markers.map((m) => (QUOTE_MARK.test(m) ? mark : m));
  const lines = restore(output, { ...piece, markers }, "anyOrder")?.split("\n") ?? [];
  return lines.some((line, i) => {
    /** What stands before each quote mark of the line. */
    const before = line.split(mark).slice(0, -1);
    return i === 0 ? before.length > 0 : before.some((text) => !/^[ \t>]*$/.test(text));
  });
}

// `text` without the one pair of quotation marks around all of it; `null`: none, or a mark of the pair inside too
// (quoted parts of a sentence that happen to open and close it)
function unquoted(text: string): string | null {
  for (const [open, close] of QUOTES) {
    if (text.length < 2 || !text.startsWith(open) || !text.endsWith(close)) continue;
    const inner = text.slice(open.length, text.length - close.length);
    if (!inner.includes(open) && !inner.includes(close)) return inner.trim();
  }
  return null;
}

// `answer` says what `original` (the piece as written) says already: the same text, or, when the run reads the source
// off the text (`sourceRead`) and the piece is long enough for it to say something (`NEAR_SAME_MIN_CHARS`), nearly
// the same (`NEAR_SAME`)
function isThePiece(answer: string, original: string, sourceRead: boolean): boolean {
  const said = folded(answer);
  const written = folded(original);
  if (said === written) return true;
  if (!sourceRead) return false;
  const least = mostlyCjk(written) ? NEAR_SAME_MIN_CHARS.dense : NEAR_SAME_MIN_CHARS.alphabetic;
  return written.length >= least && similarity(said, written) >= NEAR_SAME;
}

// what two texts are compared by: NFC, any run of whitespace as one space, none around
function folded(text: string): string {
  return text.normalize("NFC").replace(/\s+/gu, " ").trim();
}

// under `MIN_SCRIPT_SHARE` of the letters of `answer` are in the script of `target`, the words it copied from
// `source` (the piece's text) not counted; false when the runtime does not know that script, or when no letter is
// left to count (nothing to hold the answer to)
function offScript(answer: string, source: string, target: string): boolean {
  const code = scriptOf(target);
  if (code === undefined) return false;
  const scripts = SCRIPTS_OF[code] ?? [code];
  try {
    const script = new RegExp(
      `[${scripts.map((s) => `\\p{Script_Extensions=${s}}`).join("")}]`,
      "u",
    );
    const own = withoutCopies(answer, source, script);
    return LETTER.test(own) && letterShare(own, script) < MIN_SCRIPT_SHARE;
  } catch {
    return false;
  }
}

// `answer` without the words it copied from `source`: a word (`WORD`) that stands there exactly as here, letter
// case included. A word with a char of the target's script (`script`: a pattern for one char) stays whatever the
// piece holds: it counts for the target as it is.
function withoutCopies(answer: string, source: string, script: RegExp): string {
  const copied = new Set(source.match(WORD));
  return answer.replace(WORD, (word) => (copied.has(word) && !script.test(word) ? "" : word));
}

// `text` on one line: each line break, with the space around it, is one space; nothing between two chars of a script
// written without spaces
function oneLine(text: string): string {
  return text
    .split(/\s*\n\s*/)
    .reduce(
      (line, next) =>
        line + (ENDS_UNSPACED.test(line) && STARTS_UNSPACED.test(next) ? "" : " ") + next,
    );
}
