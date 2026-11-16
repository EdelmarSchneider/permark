// The words of a Markdown text worth a spelling check, with their UTF-16 offsets in that text (the editors' text: Edit
// and Source hold the same one). What is not prose is masked first (replaced by spaces, so offsets stay): code, math,
// HTML, link | image destinations and titles, autolinks and bare URLs | emails (the grammar's GFM autolinks), the
// front matter block, wikilink targets, footnote labels, tags. Then a whitespace chunk that reads as a path, an
// address or an identifier (`/`, `\`, `@`, `_`, a dot between letters) is masked whole. Words come from the word
// segmenter (UAX #29); an apostrophe | middle dot between letters stays inside the word (fr | it | ca elision, ca
// `l·l`). A hyphenated word comes whole with its parts: the whole is accepted when known, else each unknown part is
// marked. Words no dictionary can judge are left out: with digits, in capitals, camelCase, single letters, scripts
// with no Hunspell dictionary (Han, kana, Hangul).
// A large document is read in bounded steps (`tokenSteps`): the grammar parses one block per step, then the words
// come slice by slice. `tokenizeInSteps` runs those steps a few milliseconds at a time and lets the page breathe in
// between, so typing stays smooth while a long text is read.

import { markdownLanguage } from "@codemirror/lang-markdown";

/** A word of the text (`text.slice(from, to) === word`). */
export type Word = { from: number; to: number; word: string };

/** A word to check; `parts`: the parts of a hyphenated word that can be checked alone (empty for other words). */
export type Token = Word & { parts: readonly Word[] };

type Range = { from: number; to: number };
type Tree = ReturnType<typeof markdownLanguage.parser.parse>;

/** Most characters of prose sent to guess a document's language (what the backend reads of a sample). */
export const SAMPLE_CHARS = 8192;
/** About how many characters one step of the word phase reads (it ends at the next line break | space). */
export const SLICE_CHARS = 16_384;
/** Longest run of steps before `tokenizeInSteps` pauses (ms): well under a frame, so keys are never held up. */
export const RUN_MS = 8;

// grammar nodes that are no prose at all
const MASKED_NODES = new Set([
  "FencedCode",
  "CodeBlock",
  "InlineCode",
  "HTMLBlock",
  "HTMLTag",
  "CommentBlock",
  "Comment",
  "ProcessingInstructionBlock",
  "ProcessingInstruction",
  "LinkReference",
  "LinkLabel",
  "URL",
  "Autolink",
  "Entity",
  "Emoji",
  "Escape",
]);

// syntax the grammar does not know, read from the text: `$$…$$` display math (may span lines), `$…$` inline math
// (comrak `math_dollars`: no space inside the dollars, no digit right after), footnote labels `[^x]`, tags `#tag`
const MASKED_TEXT = [
  /\$\$[\s\S]+?\$\$/g,
  /\$(?!\s)[^$\n]+(?<!\s)\$(?!\d)/g,
  /\[\^[^\]\s]+\]/g,
  /(?<![\p{L}\p{N}_&#/])#[\p{L}\p{N}_][\p{L}\p{N}_/-]*/gu,
];
// a wikilink: the target (group 1) is masked, an alias after `|` is prose
const WIKILINK = /\[\[([^\]|\n]+)(\|[^\]\n]*)?\]\]/g;
// a closed `---` | `+++` block at the very top (`...` also closes YAML)
const FRONT_MATTER = /^(---|\+\+\+)[ \t]*\r?\n(?:[^\n]*\n)*?(?:\1|\.\.\.)[ \t]*(?:\r?\n|$)/;
// its first line reads as a key (`key:` YAML, `key =` TOML), a TOML table or a comment: a document may also open
// with a thematic break `---`, whose prose up to the next one is no front matter
const KEY_LINE = /^[^\n]*\n[ \t]*(?:#|\[|[^\s:=#][^\n:=]*(?::(?=\s|$)|[ \t]*=))/;
// a chunk (whitespace-separated, outer punctuation off) that reads as a path, an address or an identifier
const CODE_LIKE = /[/\\@_]|[\p{L}\p{N}]\.[\p{L}\p{N}]/u;
const OUTER_PUNCTUATION = /^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu;
const APOSTROPHES = new Set(["'", "’", "·"]);
const HYPHENS = new Set(["-", "‐"]);
const LETTER = /\p{L}/gu;
const DIGIT = /\p{N}/u;
const CAMEL = /\p{Ll}\p{Lu}/u;
const UNSUPPORTED_SCRIPT =
  /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;

/** Built on the first use (the plugin loads nothing before a document is edited). */
let segmenter: Intl.Segmenter | null = null;

/** The words of `text` to check, in document order, read in one go (`slice`: characters per word step). */
export function tokenize(text: string, slice = SLICE_CHARS): Token[] {
  const steps = tokenSteps(text, slice);
  for (;;) {
    const step = steps.next();
    if (step.done === true) return step.value;
  }
}

/**
 * `tokenize` in runs of at most `RUN_MS`, waiting for `pause()` between two runs; `null`: `stale()` said, after a
 * pause, that the words are no longer wanted. A small text is read in one run (no pause).
 */
export async function tokenizeInSteps(
  text: string,
  stale: () => boolean,
  pause: () => Promise<void> = nextIdle,
): Promise<Token[] | null> {
  const steps = tokenSteps(text);
  let start = performance.now();
  for (;;) {
    const step = steps.next();
    if (step.done === true) return step.value;
    if (performance.now() - start < RUN_MS) continue;
    await pause();
    if (stale()) return null;
    start = performance.now();
  }
}

/**
 * The reading of `text` one bounded step at a time; each step yields the offset it reached (from 0 again when the
 * word phase begins). The grammar parses one block per step. The extra rules then scan the whole text natively
 * (about a millisecond per megabyte). Last, the words come slice by slice of about `slice` characters; a slice ends
 * after a line break (else a space), where no word, joiner or whitespace chunk spans the cut, and the masked ranges
 * come from the whole text: the words are the same as read in one slice.
 */
export function* tokenSteps(text: string, slice = SLICE_CHARS): Generator<number, Token[]> {
  const parse = markdownLanguage.parser.startParse(text);
  let tree = parse.advance();
  while (tree === null) {
    yield parse.parsedPos;
    tree = parse.advance();
  }
  const extra = textRanges(text);
  const out: Token[] = [];
  for (let from = 0; from < text.length;) {
    const to = sliceEnd(text, from, slice);
    const ranges = treeRanges(text, tree, from, to);
    for (const range of extra) if (range.from < to && range.to > from) ranges.push(range);
    wordsOf(text, from, maskChunks(mask(text, from, to, ranges)), out);
    yield to;
    from = to;
  }
  return out;
}

/** The end of the slice that starts at `from`: after the first line break past `size` characters (a space when no
 *  line break is near), or the end of the text. */
function sliceEnd(text: string, from: number, size: number): number {
  const at = from + size;
  if (at >= text.length) return text.length;
  const line = text.indexOf("\n", at);
  if (line !== -1 && line - at < size) return line + 1;
  const space = text.indexOf(" ", at);
  return space === -1 ? text.length : space + 1;
}

/** Resolves once the page is idle (no `requestIdleCallback`, as in some webviews: on the next task). */
function nextIdle(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestIdleCallback === "function") {
      requestIdleCallback(() => resolve(), { timeout: 100 });
    } else setTimeout(resolve, 0);
  });
}

/** Adds the tokens of `masked` (the slice of `text` at `offset`, no prose masked) to `out`. */
function wordsOf(text: string, offset: number, masked: string, out: Token[]): void {
  segmenter ??= new Intl.Segmenter(undefined, { granularity: "word" });
  const segments = [...segmenter.segment(masked)];
  for (let i = 0; i < segments.length; i += 1) {
    const first = segments[i];
    if (first?.isWordLike !== true) continue;
    // a run of word segments joined by apostrophes (inside a part) | hyphens (between parts)
    const parts: Word[] = [wordAt(text, offset + first.index, first.segment.length)];
    while (true) {
      const joiner = segments[i + 1];
      const next = segments[i + 2];
      if (joiner === undefined || next?.isWordLike !== true) break;
      const last = parts[parts.length - 1];
      if (last === undefined) break;
      if (APOSTROPHES.has(joiner.segment)) {
        parts[parts.length - 1] = wordAt(
          text,
          last.from,
          offset + next.index + next.segment.length - last.from,
        );
      } else if (HYPHENS.has(joiner.segment)) {
        parts.push(wordAt(text, offset + next.index, next.segment.length));
      } else break;
      i += 2;
    }
    const head = parts[0];
    const tail = parts[parts.length - 1];
    if (head === undefined || tail === undefined) continue;
    const whole = wordAt(text, head.from, tail.to - head.from);
    if (parts.length === 1) {
      if (checkable(whole.word)) out.push({ ...whole, parts: [] });
      continue;
    }
    // a hyphenated word with no part to mark when unknown: nothing to say about it
    const own = parts.filter((p) => checkable(p.word));
    if (own.length > 0 && checkable(whole.word, true)) out.push({ ...whole, parts: own });
  }
}

/** The prose of the tokens as one sample for the language guess (capped). */
export function sampleOf(tokens: readonly Token[]): string {
  let sample = "";
  for (const token of tokens) {
    if (sample.length + token.word.length + 1 > SAMPLE_CHARS) break;
    sample += sample === "" ? token.word : ` ${token.word}`;
  }
  return sample;
}

function wordAt(text: string, from: number, length: number): Word {
  return { from, to: from + length, word: text.slice(from, from + length) };
}

/** Whether a dictionary can judge `word` (`joined`: a hyphenated whole, whose single-letter parts are fine). */
function checkable(word: string, joined = false): boolean {
  const letters = word.match(LETTER)?.length ?? 0;
  if (letters < (joined ? 1 : 2)) return false;
  if (DIGIT.test(word) || UNSUPPORTED_SCRIPT.test(word) || CAMEL.test(word)) return false;
  return !(letters >= 2 && word === word.toUpperCase() && word !== word.toLowerCase());
}

/** The ranges of `text` between `from` and `to` that are no prose, as the grammar reads them (`tree`). */
function treeRanges(text: string, tree: Tree, from: number, to: number): Range[] {
  const found: Range[] = [];
  tree.iterate({
    from,
    to,
    enter(node) {
      if (MASKED_NODES.has(node.name)) {
        found.push({ from: node.from, to: node.to });
        return false;
      }
      if (node.name !== "Link" && node.name !== "Image") return undefined;
      // a footnote reference | label written as a link: all of it
      if (text.startsWith("[^", node.from)) {
        found.push({ from: node.from, to: node.to });
        return false;
      }
      // the destination and title `(…)`; the link text stays prose
      for (let child = node.node.firstChild; child; child = child.nextSibling) {
        if (child.name === "LinkMark" && text[child.from] === "(") {
          found.push({ from: child.from, to: node.to });
          break;
        }
      }
      return undefined;
    },
  });
  return found;
}

/** The ranges of `text` that are no prose by the rules the grammar does not know (front matter, math, …). */
function textRanges(text: string): Range[] {
  const found: Range[] = [];
  const front = KEY_LINE.test(text) ? FRONT_MATTER.exec(text) : null;
  if (front !== null) found.push({ from: 0, to: front[0].length });
  for (const pattern of MASKED_TEXT) {
    for (const match of text.matchAll(pattern)) {
      found.push({ from: match.index, to: match.index + match[0].length });
    }
  }
  for (const match of text.matchAll(WIKILINK)) {
    const target = match[1] ?? "";
    const alias = match[2];
    const end = match.index + match[0].length;
    // `[[target` and the `|`; without an alias the closing `]]` too
    found.push({ from: match.index, to: match.index + 2 + target.length + (alias ? 1 : 2) });
    if (alias) found.push({ from: end - 2, to: end });
  }
  return found;
}

/** `text` from `start` to `end` with `ranges` (offsets of `text`) replaced by spaces (same length: offsets stay). */
function mask(text: string, start: number, end: number, ranges: readonly Range[]): string {
  if (ranges.length === 0) return text.slice(start, end);
  const sorted = [...ranges].sort((a, b) => a.from - b.from);
  let out = "";
  let at = start;
  for (const range of sorted) {
    const from = Math.max(range.from, at);
    const to = Math.min(range.to, end);
    if (to <= from) continue;
    out += text.slice(at, from) + " ".repeat(to - from);
    at = to;
  }
  return out + text.slice(at, end);
}

/** `masked` with every chunk that reads as a path | address | identifier masked too. */
function maskChunks(masked: string): string {
  const ranges: Range[] = [];
  for (const match of masked.matchAll(/\S+/g)) {
    if (CODE_LIKE.test(match[0].replace(OUTER_PUNCTUATION, ""))) {
      ranges.push({ from: match.index, to: match.index + match[0].length });
    }
  }
  return mask(masked, 0, masked.length, ranges);
}
