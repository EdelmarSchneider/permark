// Word-level suggestions between a piece's original text and the model's corrected text (spec feature-correction
// D-6). Tokens = `Intl.Segmenter` word segments (never split a grapheme), whitespace runs merged; compared NFC, every
// whitespace run equal to any other (model reflow invisible, original spaces + EOL kept), so a space added | dropped
// inside a line stays an edit. Myers = `core/editing/lines.ts diff` over the token keys. Changes one whitespace token
// or one short token apart -> one suggestion. Tokens equal under NFC keep the original bytes.

import { diff, type DiffOp } from "../../core/editing/lines";

/** Context chars kept on each side of a suggestion (to find it again, to show it). */
export const CONTEXT_CHARS = 40;
/** An equal token at most this long between two changes joins them. */
const SHORT_EQUAL = 3;

export type Token = {
  text: string;
  /** UTF-16 offset in the tokenized text. */
  at: number;
  /** Whitespace run. */
  space: boolean;
  /** Word (letters | numbers), not punctuation | space. */
  word: boolean;
};

export type WordDiff = { a: readonly Token[]; b: readonly Token[]; ops: readonly DiffOp[] };

export type Suggestion = {
  /** Document range (UTF-16) replaced. */
  from: number;
  to: number;
  /** Text there when found (`doc.slice(from, to)`). */
  original: string;
  insert: string;
  /** Up to `CONTEXT_CHARS` of document text before | after the range. */
  before: string;
  after: string;
};

const SPACE = /^\s+$/u;

export function tokenize(text: string, lang?: string): Token[] {
  const out: Token[] = [];
  for (const s of new Intl.Segmenter(lang, { granularity: "word" }).segment(text)) {
    const space = SPACE.test(s.segment);
    const last = out.at(-1);
    if (space && last?.space) last.text += s.segment;
    else out.push({ text: s.segment, at: s.index, space, word: s.isWordLike === true });
  }
  return out;
}

function key(token: Token): string {
  return token.space ? " " : token.text.normalize("NFC");
}

/** Token diff of `original` -> `corrected`; leading | trailing space of `corrected` replaced by the original's. */
export function wordDiff(original: string, corrected: string, lang?: string): WordDiff {
  const lead = /^\s*/u.exec(original)?.[0] ?? "";
  const trail = /\s*$/u.exec(original)?.[0] ?? "";
  const a = tokenize(original, lang);
  const b = tokenize(lead + corrected.trim() + trail, lang);
  return { a, b, ops: diff(a.map(key), b.map(key)) };
}

type Run = { ops: DiffOp[]; a: number; aEnd: number };

/** Suggestions of `d` for its original text found at `at` in `doc`, in document order. */
export function suggest(doc: string, at: number, d: WordDiff): Suggestion[] {
  const runs: Run[] = [];
  let open: Run | null = null;
  d.ops.forEach((op, i) => {
    if (op.kind === "same") {
      if (open !== null && !(joins(d, op) && i + 1 < d.ops.length)) open = null;
      else open?.ops.push(op);
      return;
    }
    if (open === null) {
      open = { ops: [], a: op.a, aEnd: op.a };
      runs.push(open);
    }
    open.ops.push(op);
    open.aEnd = op.a + op.remove;
  });
  const out: Suggestion[] = [];
  for (const run of runs) {
    const from = at + offsetOf(d.a, run.a);
    const to = at + offsetOf(d.a, run.aEnd);
    const insert = run.ops.map((op) => replacement(d, op)).join("");
    const original = doc.slice(from, to);
    if (insert === original) continue;
    out.push({
      from,
      to,
      original,
      insert,
      ...around(doc, from, to),
    });
  }
  return out;
}

// equal run short enough to join the changes around it
function joins(d: WordDiff, op: Extract<DiffOp, { kind: "same" }>): boolean {
  const token = d.a[op.a];
  return (
    op.count === 1 && token !== undefined && (token.space || [...token.text].length <= SHORT_EQUAL)
  );
}

function replacement(d: WordDiff, op: DiffOp): string {
  if (op.kind === "same") return textOf(d.a, op.a, op.a + op.count);
  return textOf(d.b, op.b, op.b + op.add);
}

function textOf(tokens: readonly Token[], from: number, to: number): string {
  return tokens
    .slice(from, to)
    .map((t) => t.text)
    .join("");
}

function offsetOf(tokens: readonly Token[], index: number): number {
  const token = tokens[index];
  if (token !== undefined) return token.at;
  const last = tokens.at(-1);
  return last === undefined ? 0 : last.at + last.text.length;
}

// slice that never starts | ends inside a surrogate pair
/** The `CONTEXT_CHARS` of `doc` on each side of `from..to` (a suggestion's excerpt; never half a surrogate pair). */
export function around(doc: string, from: number, to: number): { before: string; after: string } {
  return {
    before: context(doc, from - CONTEXT_CHARS, from),
    after: context(doc, to, to + CONTEXT_CHARS),
  };
}

function context(doc: string, from: number, to: number): string {
  let start = Math.max(0, from);
  let end = Math.min(doc.length, to);
  if (isLow(doc.charCodeAt(start)) && start > 0 && start < end) start += 1;
  if (end < doc.length && isLow(doc.charCodeAt(end)) && end > start) end -= 1;
  return doc.slice(start, end);
}

function isLow(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}
