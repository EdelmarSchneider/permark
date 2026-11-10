// One change a model proposes for the open document: a range of its text, what goes there, and the excerpt on each
// side (to show it, to find it again). What builds it is the plugin's (correction: one per changed word run).

/** Context chars kept on each side of a suggestion (to find it again, to show it). */
export const CONTEXT_CHARS = 40;
/** A blank line: an excerpt never crosses it. */
const BLANK_LINE = /\n[ \t]*\n/g;
/** Marks an excerpt cut short. */
const CUT = "…";

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

/** The excerpt of `doc` on each side of `from..to`: up to `CONTEXT_CHARS`, never past a blank line, a size cut moved
 *  to a word edge (no space to cut at, as in CJK: kept, never half a surrogate pair) and marked `…`. */
export function around(doc: string, from: number, to: number): { before: string; after: string } {
  return { before: leading(doc, from), after: trailing(doc, to) };
}

function leading(doc: string, at: number): string {
  let start = Math.max(0, at - CONTEXT_CHARS);
  // from one char earlier: a blank line right across the cut
  const blank = [...doc.slice(Math.max(0, start - 1), at).matchAll(BLANK_LINE)].at(-1);
  if (blank !== undefined)
    return doc.slice(Math.max(0, start - 1) + blank.index + blank[0].length, at);
  if (start === 0) return doc.slice(0, at);
  if (!isSpace(doc[start - 1]) && !isSpace(doc[start])) {
    const space = doc.slice(start, at).search(/\s/u);
    if (space >= 0) start += space;
    else if (isLow(doc.charCodeAt(start)) && start < at) start += 1;
  }
  return CUT + doc.slice(start, at).replace(/^\s+(?=\S)/u, "");
}

function trailing(doc: string, at: number): string {
  let end = Math.min(doc.length, at + CONTEXT_CHARS);
  const blank = doc.slice(at, Math.min(doc.length, end + 1)).search(BLANK_LINE);
  if (blank >= 0) return doc.slice(at, at + blank);
  if (end === doc.length) return doc.slice(at);
  if (!isSpace(doc[end - 1]) && !isSpace(doc[end])) {
    const space = doc.slice(at, end).search(/\s\S*$/u);
    if (space > 0) end = at + space;
    else if (isLow(doc.charCodeAt(end)) && end > at) end -= 1;
  }
  return doc.slice(at, end).replace(/(?<=\S)\s+$/u, "") + CUT;
}

function isSpace(ch: string | undefined): boolean {
  return ch !== undefined && /\s/u.test(ch);
}

function isLow(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}
