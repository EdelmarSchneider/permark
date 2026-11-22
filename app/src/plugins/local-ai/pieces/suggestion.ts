// One change a model proposes for the open document: a range of its text, what goes there, and the excerpt on each
// side (to show it, to find it again). What builds it is the plugin's (correction: one per changed word run). The
// excerpt of a change in a table cell is that cell's text alone: read as any other text it would run over the row's
// pipes into the cells beside it and the next row. Read as the Edit view shows the text, an excerpt leaves Markdown's
// marks out (`shownMarks`), cut where the written one is.

import { shownMarks, type PieceKind } from "./segment";

/** Context chars kept on each side of a suggestion (to find it again, to show it). */
export const CONTEXT_CHARS = 40;
/** A blank line: an excerpt never crosses it. */
const BLANK_LINE = /\n[ \t]*\n/g;
/** Marks an excerpt cut short. */
const CUT = "…";
/** Stands for each char of a mark a shown excerpt leaves out (the offsets stay those of the text); dropped from what
 *  the excerpt says. */
const LEFT_OUT = "\u0000";

/** How an excerpt reads the text: as written (Source) | as the Edit view shows it, Markdown's marks left out. */
export type ExcerptText = "written" | "shown";

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
  /** Kind of the piece the range is text of, where what builds the suggestion knows it: the excerpt is read by it
   *  again (`around`) each time the text changed. */
  within?: PieceKind;
};

/** The excerpt of `doc` on each side of `from..to`: up to `CONTEXT_CHARS`, never past a blank line, a size cut moved
 *  to a word edge (no space to cut at, as in CJK: kept, never half a surrogate pair) and marked `…`. `within` a
 *  table `cell`: never past the cell either (its pipes, its padding); any other kind, or none: as said. `shown`:
 *  the same cut, its text without the marks the Edit view draws as looks (`**`, a quote's `>`, a link's target). */
export function around(
  doc: string,
  from: number,
  to: number,
  within?: PieceKind,
  as: ExcerptText = "written",
): { before: string; after: string } {
  if (as === "written") return excerpt(doc, doc, from, to, within);
  const lines = linesAround(doc, from, to);
  const at = from - lines.start;
  return excerpt(masked(lines.text), lines.text, at, at + to - from, within);
}

/** The excerpt read in `text`; a table cell's edges found in `written` (the same text with every mark: an escaped
 *  pipe stays the cell's own). */
function excerpt(
  text: string,
  written: string,
  from: number,
  to: number,
  within?: PieceKind,
): { before: string; after: string } {
  if (within !== "cell") return { before: leading(text, from), after: trailing(text, to) };
  const cell = cellAround(written, from, to);
  const before = bare(text.slice(cell.from, from)).trimStart();
  return {
    before: leading(before, before.length),
    after: trailing(bare(text.slice(to, cell.to)).trimEnd(), 0),
  };
}

/** The whole lines of `doc` an excerpt of `from..to` may reach, one char more each way (the blank line check): read
 *  alone, their Markdown reads as in the document, but for a run opened further up. */
function linesAround(doc: string, from: number, to: number): { text: string; start: number } {
  const reach = from - CONTEXT_CHARS - 2;
  const start = reach < 0 ? 0 : doc.lastIndexOf("\n", reach) + 1;
  const next = doc.indexOf("\n", Math.min(doc.length, to + CONTEXT_CHARS + 1));
  return { text: doc.slice(start, next < 0 ? doc.length : next), start };
}

/** `text` with each char of its shown marks as `LEFT_OUT` (UTF-16 units: offsets unchanged). */
function masked(text: string): string {
  const units = text.split("");
  for (const { from, to } of shownMarks(text)) units.fill(LEFT_OUT, from, to);
  return units.join("");
}

/** What an excerpt says of `text`: the marks left out gone. */
function bare(text: string): string {
  return text.replaceAll(LEFT_OUT, "");
}

/** The table cell that holds `from..to`, padding included: from the pipe before it on its line to the pipe after it;
 *  a row written without an outer pipe: from the line's start (past a quote's marks and the indent) | to its end.
 *  Pipes as the table grammar reads a row: one after a backslash is the cell's text, `\\|` is a backslash, then a
 *  pipe. */
function cellAround(doc: string, from: number, to: number): { from: number; to: number } {
  const lineStart = doc.lastIndexOf("\n", Math.max(0, from - 1)) + 1;
  const lineEnd = doc.indexOf("\n", to);
  let start = lineStart + (/^[ \t>]*/.exec(doc.slice(lineStart, from))?.[0].length ?? 0);
  let end = lineEnd < 0 ? doc.length : lineEnd;
  let escaped = false;
  for (let i = lineStart; i < end; i++) {
    const ch = doc[i];
    if (ch === "|" && !escaped) {
      if (i < from) start = i + 1;
      else if (i >= to) end = i;
    }
    escaped = !escaped && ch === "\\";
  }
  return { from: start, to: end };
}

function leading(doc: string, at: number): string {
  let start = Math.max(0, at - CONTEXT_CHARS);
  // from one char earlier: a blank line right across the cut
  const blank = [...doc.slice(Math.max(0, start - 1), at).matchAll(BLANK_LINE)].at(-1);
  if (blank !== undefined)
    return bare(doc.slice(Math.max(0, start - 1) + blank.index + blank[0].length, at));
  if (start === 0) return bare(doc.slice(0, at));
  if (!isSpace(doc[start - 1]) && !isSpace(doc[start])) {
    const space = doc.slice(start, at).search(/\s/u);
    if (space >= 0) start += space;
    else if (isLow(doc.charCodeAt(start)) && start < at) start += 1;
  }
  return CUT + bare(doc.slice(start, at)).replace(/^\s+(?=\S)/u, "");
}

function trailing(doc: string, at: number): string {
  let end = Math.min(doc.length, at + CONTEXT_CHARS);
  const blank = doc.slice(at, Math.min(doc.length, end + 1)).search(BLANK_LINE);
  if (blank >= 0) return bare(doc.slice(at, at + blank));
  if (end === doc.length) return bare(doc.slice(at));
  if (!isSpace(doc[end - 1]) && !isSpace(doc[end])) {
    const space = doc.slice(at, end).search(/\s\S*$/u);
    if (space > 0) end = at + space;
    else if (isLow(doc.charCodeAt(end)) && end > at) end -= 1;
  }
  return bare(doc.slice(at, end)).replace(/(?<=\S)\s+$/u, "") + CUT;
}

function isSpace(ch: string | undefined): boolean {
  return ch !== undefined && /\s/u.test(ch);
}

function isLow(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}
