// The words of a Markdown text worth a spelling check, with their UTF-16 offsets in that text (the editors' text: Edit
// and Source hold the same one). What is not prose is masked first (replaced by spaces, so offsets stay): code, math,
// HTML, link | image destinations and titles, autolinks and bare URLs | emails (the grammar's GFM autolinks), the
// front matter block, wikilink targets, footnote labels, tags. Then a whitespace chunk that reads as a path, an
// address or an identifier (`/`, `\`, `@`, `_`, a dot between letters) is masked whole. Words come from the word
// segmenter (UAX #29); an apostrophe | middle dot between letters stays inside the word (fr | it | ca elision, ca
// `l·l`). A hyphenated word comes whole with its parts: the whole is accepted when known, else each unknown part is
// marked. Words no dictionary can judge are left out: with digits, in capitals, camelCase, single letters, scripts
// with no Hunspell dictionary (Han, kana, Hangul).

import { markdownLanguage } from "@codemirror/lang-markdown";

/** A word of the text (`text.slice(from, to) === word`). */
export type Word = { from: number; to: number; word: string };

/** A word to check; `parts`: the parts of a hyphenated word that can be checked alone (empty for other words). */
export type Token = Word & { parts: readonly Word[] };

type Range = { from: number; to: number };

/** Most characters of prose sent to guess a document's language (what the backend reads of a sample). */
export const SAMPLE_CHARS = 8192;

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

/** The words of `text` to check, in document order. */
export function tokenize(text: string): Token[] {
  const masked = maskChunks(mask(text, syntaxRanges(text)));
  segmenter ??= new Intl.Segmenter(undefined, { granularity: "word" });
  const segments = [...segmenter.segment(masked)];
  const out: Token[] = [];
  for (let i = 0; i < segments.length; i += 1) {
    const first = segments[i];
    if (first?.isWordLike !== true) continue;
    // a run of word segments joined by apostrophes (inside a part) | hyphens (between parts)
    const parts: Word[] = [wordAt(text, first.index, first.segment.length)];
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
          next.index + next.segment.length - last.from,
        );
      } else if (HYPHENS.has(joiner.segment)) {
        parts.push(wordAt(text, next.index, next.segment.length));
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
  return out;
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

/** The ranges of `text` that are no prose, as the grammar and the extra rules read them. */
function syntaxRanges(text: string): Range[] {
  const found: Range[] = [];
  const front = FRONT_MATTER.exec(text);
  if (front !== null) found.push({ from: 0, to: front[0].length });
  markdownLanguage.parser.parse(text).iterate({
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

/** `text` with `ranges` replaced by spaces (same length: offsets stay). */
function mask(text: string, ranges: readonly Range[]): string {
  if (ranges.length === 0) return text;
  const sorted = [...ranges].sort((a, b) => a.from - b.from);
  let out = "";
  let at = 0;
  for (const { from, to } of sorted) {
    const start = Math.max(from, at);
    if (to <= start) continue;
    out += text.slice(at, start) + " ".repeat(to - start);
    at = to;
  }
  return out + text.slice(at);
}

/** `masked` with every chunk that reads as a path | address | identifier masked too. */
function maskChunks(masked: string): string {
  const ranges: Range[] = [];
  for (const match of masked.matchAll(/\S+/g)) {
    if (CODE_LIKE.test(match[0].replace(OUTER_PUNCTUATION, ""))) {
      ranges.push({ from: match.index, to: match.index + match[0].length });
    }
  }
  return mask(masked, ranges);
}
