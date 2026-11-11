// Pieces of a Markdown document to send to a model, one at a time (a plugin's run: correction). Parsed with the
// editor's own grammar (Markdown + GFM in YAML front matter, as `languageFor` in `core/editing/source/cm.ts`) so the
// UTF-16 offsets are the editor's. Piece = one prose block (paragraph | heading | list item | quote paragraph | the
// text of one table cell), clipped to the scope; long blocks -> sentence groups under a char budget. Whole blocks
// never sent: front matter, code, HTML, comments, `$$` math, link definitions. Of a table only each cell's own text
// goes: never a pipe, the padding or the alignment row; a table the grammar does not read as one is left alone. Of a
// quote alert (`> [!NOTE]`) the marker is no part of a piece: an answer cannot rewrite it and undo the alert.
// Inline non-prose -> numbered markers `⟦n⟧` the model must copy back, each once (in the piece's order, or anywhere
// for a plugin whose answers move the words); `restore` puts the original bytes back.

import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { yamlFrontmatter } from "@codemirror/lang-yaml";
import { frontMatterSyntax } from "../../../core/config/frontMatter";

type Parser = ReturnType<typeof yamlFrontmatter>["language"]["parser"];
type Node = ReturnType<Parser["parse"]>["topNode"];

/** Built on the first `pieces` (never at app start: the plugin stays dormant until a run). */
let parser: Parser | null = null;

function markdownParser(): Parser {
  parser ??= yamlFrontmatter({ content: markdown({ base: markdownLanguage }) }).language.parser;
  return parser;
}

/** Chars per piece (~400 tokens) for alphabetic text. */
export const CHAR_BUDGET = 1200;
/** Chars per piece when the block is mostly Han | Kana | Hangul (about one token per char). */
export const CJK_CHAR_BUDGET = 400;
/** Share of CJK letters past which a block counts as CJK. */
const CJK_SHARE = 0.5;

/** Chars a piece may hold: alphabetic text | text mostly Han | Kana | Hangul. */
export type Budget = { chars: number; cjkChars: number };

export type Scope = { from: number; to: number };

export type PieceKind = "paragraph" | "heading" | "listItem" | "quote" | "cell";

export type Piece = {
  /** Document range (UTF-16) of the piece. */
  from: number;
  to: number;
  kind: PieceKind;
  /** `doc.slice(from, to)` when cut (snapshot). */
  original: string;
  /** What the model gets: `original` with inline non-prose replaced by `⟦1⟧…⟦n⟧`. */
  text: string;
  /** Original bytes of marker `⟦i + 1⟧`. */
  markers: readonly string[];
};

/** Where the markers of an answer may stand: as in the piece | anywhere (an answer that moves the words around
 *  them: a translation). */
export type MarkerOrder = "inOrder" | "anyOrder";

type Range = { from: number; to: number };

const MARKER = /⟦(\d+)⟧/g;
const LETTER = /\p{L}/u;
const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;
const SPACE = /\s/u;

// inline nodes hidden whole
const HIDDEN = new Set([
  "InlineCode",
  "URL",
  "Autolink",
  "HTMLTag",
  "Comment",
  "ProcessingInstruction",
  "HardBreak",
  "Entity",
  "Emoji",
  "TaskMarker",
  "QuoteMark",
  "LinkLabel",
]);
// inline syntax the grammar does not know: wikilinks, `$…$` | `$$…$$` math (comrak `math_dollars`), literal markers
const HIDDEN_TEXT = [
  /\[\[[^\]\n]+\]\]/g,
  /\$\$[^$]+\$\$/g,
  /\$(?!\s)[^$\n]+(?<!\s)\$(?!\d)/g,
  /[⟦⟧]/g,
];
// the marker of a quote alert: the five kinds the reader draws, in any letter case
const ALERT_MARKER = /^\[!(?:note|tip|important|warning|caution)\]/i;
// a table's alignment row inside a paragraph: a table the reader may draw that the grammar did not read as one
// (spaces | `\r` after the row)
const ALIGNMENT_ROW =
  /^[ \t>]*(?:\|[ \t]*)?:?-+:?[ \t]*\|(?:[ \t]*:?-+:?[ \t]*\|)*(?:[ \t]*:?-+:?)?[ \t\r]*$/m;

/** Pieces of `doc` to send, in document order; `scope` limits them (UTF-16 offsets; default whole document), `budget`
 *  their size (default: what a small model takes well). */
export function pieces(
  doc: string,
  scope?: Scope,
  budget: Budget = { chars: CHAR_BUDGET, cjkChars: CJK_CHAR_BUDGET },
): Piece[] {
  const tree = markdownParser().parse(doc);
  const skipUntil = frontMatterEnd(doc);
  const out: Piece[] = [];
  /** End of the `$$` display math being skipped. */
  let mathEnd = 0;
  tree.iterate({
    enter(ref) {
      if (ref.name === "Frontmatter") return false;
      if (!isProse(ref.name)) return undefined;
      if (ref.from < skipUntil || ref.from < mathEnd) return false;
      const node = ref.node;
      // `$$` display math the grammar sees as paragraphs (may hold blank lines) up to its closing `$$`; unclosed ->
      // only its own paragraph
      if (ref.name === "Paragraph" && doc.startsWith("$$", node.from)) {
        const close = doc.indexOf("$$", node.from + 2);
        mathEnd = close < 0 ? node.to : close + 2;
        return false;
      }
      // a table the grammar missed: left alone (as a paragraph its pipes would go to the model as words)
      if (ref.name === "Paragraph" && ALIGNMENT_ROW.test(doc.slice(node.from, node.to))) {
        return false;
      }
      const content = trim(doc, contentRange(doc, node));
      if (content.from >= content.to) return false;
      const hidden = hiddenRanges(doc, node, content);
      const clipped = clip(doc, content, hidden, scope);
      if (clipped === null) return false;
      for (const group of groups(doc, clipped, hidden, budget)) {
        const piece = cut(doc, group, hidden, kindOf(node));
        if (piece !== null) out.push(piece);
      }
      return false;
    },
  });
  return out;
}

/** `output` with the markers back as original bytes; null unless it holds each marker `⟦1⟧…⟦n⟧` of the piece exactly
 *  once (`inOrder`, the default: in the piece's order too) and no other marker bracket. */
export function restore(
  output: string,
  piece: Piece,
  order: MarkerOrder = "inOrder",
): string | null {
  const seen: number[] = [];
  const back = output.replace(MARKER, (_, digits: string) => {
    const n = Number(digits);
    seen.push(n);
    return piece.markers[n - 1] ?? "";
  });
  if (seen.length !== piece.markers.length) return null;
  const numbers = order === "inOrder" ? seen : [...seen].sort((a, b) => a - b);
  if (numbers.some((n, i) => n !== i + 1)) return null;
  if (/[⟦⟧]/.test(unmarked(output))) return null;
  return back;
}

/** `text` with its markers out: the words alone (what a length | a script is measured on). */
export function unmarked(text: string): string {
  return text.replace(MARKER, "");
}

// a `TableCell` is the cell's text alone: the grammar leaves its padding and the pipes around it out
function isProse(name: string): boolean {
  return (
    name === "Paragraph" ||
    name === "Task" ||
    name === "TableCell" ||
    /^(ATX|Setext)Heading\d$/.test(name)
  );
}

function kindOf(node: Node): PieceKind {
  if (node.name === "TableCell") return "cell";
  if (node.name.includes("Heading")) return "heading";
  if (node.name === "Task" || node.parent?.name === "ListItem") return "listItem";
  for (let up = node.parent; up; up = up.parent) if (up.name === "Blockquote") return "quote";
  return "paragraph";
}

// closed `---` | `+++` block at the very top (rule of the reader: Rust `format::markdown::frontmatter::find`)
function frontMatterEnd(doc: string): number {
  const syntax = frontMatterSyntax(doc);
  if (syntax === null) return 0;
  const lines = doc.split("\n");
  const fence = (lines[0] ?? "").trimEnd();
  if (fence !== "---" && fence !== "+++") return 0;
  let end = (lines[0] ?? "").length + 1;
  for (const line of lines.slice(1)) {
    end += line.length + 1;
    const bare = line.trimEnd();
    if (bare === fence || (syntax === "yaml" && bare === "...")) return Math.min(end, doc.length);
  }
  return 0;
}

/** Where the text of `block` starts when the block opens a quote alert as the reader takes one (comrak `alerts`):
 *  `[!NOTE]` right after the quote's first `>` and one space, what follows on that line being the alert's title.
 *  Past the marker; past the next line's quote marks too when the marker stands alone on its line. `null`: no alert. */
function alertTextStart(doc: string, block: Node): number | null {
  const quote = block.parent;
  const mark = block.prevSibling;
  if (quote?.name !== "Blockquote" || mark?.name !== "QuoteMark" || mark.from !== quote.from)
    return null;
  if (doc.slice(mark.to, block.from) !== " ") return null;
  const marker = ALERT_MARKER.exec(doc.slice(block.from, block.to));
  if (marker === null) return null;
  let from = block.from + marker[0].length;
  for (let child = block.firstChild; child; child = child.nextSibling) {
    if (child.name !== "QuoteMark" || child.from < from) continue;
    if (doc.slice(from, child.from).trim() !== "") break;
    from = child.to;
  }
  return from;
}

// the block's text without heading marks, setext underline, task box or the marker of a quote alert
function contentRange(doc: string, node: Node): Range {
  let { from, to } = node;
  from = alertTextStart(doc, node) ?? from;
  const first = node.firstChild;
  const last = node.lastChild;
  if (node.name.startsWith("ATX")) {
    if (first?.name === "HeaderMark") from = first.to;
    if (last?.name === "HeaderMark" && last.from > from) to = last.from;
  } else if (node.name.startsWith("Setext")) {
    if (last?.name === "HeaderMark") to = last.from;
  } else if (node.name === "Task" && first?.name === "TaskMarker") {
    from = first.to;
  }
  return { from, to: Math.max(from, to) };
}

function trim(doc: string, range: Range): Range {
  let { from, to } = range;
  while (from < to && SPACE.test(doc[from] ?? "")) from += 1;
  while (to > from && SPACE.test(doc[to - 1] ?? "")) to -= 1;
  return { from, to };
}

// inline non-prose inside `content`, sorted, overlaps merged
function hiddenRanges(doc: string, block: Node, content: Range): Range[] {
  const found: Range[] = [];
  const cell = block.name === "TableCell";
  const walk = (node: Node): void => {
    for (let child = node.firstChild; child; child = child.nextSibling) {
      const hidden = hiddenPart(doc, child, cell);
      if (hidden !== null) found.push(hidden);
      if (hidden === null || hidden.from > child.from) walk(child);
    }
  };
  walk(block);
  const text = doc.slice(content.from, content.to);
  for (const pattern of HIDDEN_TEXT) {
    for (const match of text.matchAll(pattern)) {
      const from = content.from + match.index;
      found.push({ from, to: from + match[0].length });
    }
  }
  found.sort((a, b) => a.from - b.from);
  const merged: Range[] = [];
  for (const range of found) {
    const from = Math.max(range.from, content.from);
    const to = Math.min(range.to, content.to);
    if (from >= to) continue;
    const last = merged.at(-1);
    if (last && from <= last.to) last.to = Math.max(last.to, to);
    else merged.push({ from, to });
  }
  return merged;
}

// part of an inline node the model never sees: whole node, or a link's destination `(…)`
function hiddenPart(doc: string, node: Node, cell: boolean): Range | null {
  if (HIDDEN.has(node.name)) return { from: node.from, to: node.to };
  if (node.name === "Escape") {
    const escaped = doc[node.from + 1];
    // backslash hard break; in a table cell an escaped pipe (the model never sees | moves a pipe of a row)
    return escaped === "\n" || (cell && escaped === "|") ? { from: node.from, to: node.to } : null;
  }
  if (node.name !== "Link" && node.name !== "Image") return null;
  // footnote reference `[^x]`
  if (doc.startsWith("[^", node.from)) return { from: node.from, to: node.to };
  for (let child = node.firstChild; child; child = child.nextSibling) {
    if (child.name === "LinkMark" && doc[child.from] === "(")
      return { from: child.from, to: node.to };
  }
  return null;
}

function inside(ranges: readonly Range[], at: number): Range | undefined {
  return ranges.find((r) => r.from < at && at < r.to);
}

// `content` cut to `scope`; a cut mid-word | mid-marker moves out to the word | marker edge
function clip(doc: string, content: Range, hidden: readonly Range[], scope?: Scope): Range | null {
  if (scope === undefined) return content;
  let from = Math.max(content.from, scope.from);
  let to = Math.min(content.to, scope.to);
  if (from >= to) return null;
  const words = new Intl.Segmenter(undefined, { granularity: "word" });
  const text = doc.slice(content.from, content.to);
  const segments = words.segment(text);
  const atFrom = segments.containing(from - content.from);
  if (atFrom?.isWordLike) from = content.from + atFrom.index;
  const atTo = segments.containing(to - content.from - 1);
  if (atTo?.isWordLike) to = content.from + atTo.index + atTo.segment.length;
  from = inside(hidden, from)?.from ?? from;
  to = inside(hidden, to)?.to ?? to;
  const range = trim(doc, { from, to });
  return range.from < range.to ? range : null;
}

// sentence groups of `range` under the budget; never cut inside a hidden range
function groups(doc: string, range: Range, hidden: readonly Range[], sizes: Budget): Range[] {
  const text = doc.slice(range.from, range.to);
  const budget = mostlyCjk(text) ? sizes.cjkChars : sizes.chars;
  if (text.length <= budget) return [range];
  const sentences = new Intl.Segmenter(undefined, { granularity: "sentence" });
  const cuts: number[] = [];
  for (const s of sentences.segment(text)) {
    const at = range.from + s.index;
    if (s.index > 0 && inside(hidden, at) === undefined) cuts.push(at);
  }
  cuts.push(range.to);
  const out: Range[] = [];
  let start = range.from;
  let last = range.from;
  for (const at of cuts) {
    if (at - start > budget && last > start) {
      out.push({ from: start, to: last });
      start = last;
    }
    last = at;
  }
  out.push({ from: start, to: range.to });
  return out.map((r) => trim(doc, r)).filter((r) => r.from < r.to);
}

/** `text` is mostly Han | Kana | Hangul: about one token per char, far fewer chars for the same words than
 *  alphabetic text. */
export function mostlyCjk(text: string): boolean {
  let letters = 0;
  let cjk = 0;
  for (const ch of text) {
    if (!LETTER.test(ch)) continue;
    letters += 1;
    if (CJK.test(ch)) cjk += 1;
  }
  return letters > 0 && cjk / letters > CJK_SHARE;
}

function cut(doc: string, range: Range, hidden: readonly Range[], kind: PieceKind): Piece | null {
  const markers: string[] = [];
  let text = "";
  let at = range.from;
  for (const h of hidden) {
    if (h.to <= range.from || h.from >= range.to) continue;
    text += doc.slice(at, h.from);
    markers.push(doc.slice(h.from, h.to));
    text += `⟦${markers.length}⟧`;
    at = h.to;
  }
  text += doc.slice(at, range.to);
  if (!LETTER.test(unmarked(text))) return null;
  return {
    from: range.from,
    to: range.to,
    kind,
    original: doc.slice(range.from, range.to),
    text,
    markers,
  };
}
