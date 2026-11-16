// Inline syntax View draws that the editor's reader takes as text: `==marked==` text, `:shortcode:` emoji and
// `^[inline notes]`. As text, Edit would show their marks; each becomes an inline piece kept as written
// (`inlineSource.ts`), drawn as View's element for it. They are found as the renderer finds them (`==` runs of exactly
// two that flank as `*` does, shortcode names, balanced brackets; never in code, math or HTML, never over a line
// break). A piece View drew otherwise (an unknown shortcode, a pairing the renderer reads differently) only shows its
// source as text: its bytes never change.

import type { Nodes, PhrasingContent, Root } from "mdast";

type Kind = "mark" | "emoji" | "note";
type Span = { from: number; to: number; kind: Kind };
/** A text child and where its source is. */
type TextAt = { from: number; to: number; source: string };
type Point = NonNullable<PhrasingContent["position"]>["start"];

const SHORTCODE = /:([A-Za-z0-9+_-]+):/g;
/** GitHub's shortcode names without a letter or `_`; any other name holds one (`12:30:45` holds no shortcode). */
const SIGN_CODES = new Set(["+1", "-1", "100", "1234"]);
const SPACE = /\s/;
const PUNCTUATION = /[\p{P}\p{S}]/u;

function offsetsOf(node: Nodes): [number, number] | null {
  const from = node.position?.start.offset;
  const to = node.position?.end.offset;
  return from === undefined || to === undefined ? null : [from, to];
}

/** The character at `at` follows an odd run of backslashes. */
function escaped(text: string, at: number): boolean {
  let slashes = 0;
  for (let i = at - 1; i >= 0 && text[i] === "\\"; i--) slashes += 1;
  return slashes % 2 === 1;
}

function emojiSpans(texts: readonly TextAt[], text: string): Span[] {
  const spans: Span[] = [];
  for (const { from, source } of texts) {
    for (const match of source.matchAll(SHORTCODE)) {
      const name = match[1] ?? "";
      const at = from + match.index;
      if (escaped(text, at) || !(/[A-Za-z_]/.test(name) || SIGN_CODES.has(name))) continue;
      spans.push({ from: at, to: at + match[0].length, kind: "emoji" });
    }
  }
  return spans;
}

/** A run of `=` that may open | close a mark: exactly two, flanking as `*` does. */
function markSpans(texts: readonly TextAt[], text: string): Span[] {
  const runs: { from: number; to: number; open: boolean; close: boolean }[] = [];
  for (const { from, source } of texts) {
    for (const match of source.matchAll(/=+/g)) {
      let start = from + match.index;
      const end = start + match[0].length;
      if (escaped(text, start)) start += 1;
      if (end - start !== 2 || text[end] === "=") continue;
      const before = text[start - 1] ?? " ";
      const after = text[end] ?? " ";
      const spaceBefore = SPACE.test(before);
      const spaceAfter = SPACE.test(after);
      const punctBefore = PUNCTUATION.test(before);
      const punctAfter = PUNCTUATION.test(after);
      runs.push({
        from: start,
        to: end,
        open: !spaceAfter && (!punctAfter || spaceBefore || punctBefore),
        close: !spaceBefore && (!punctBefore || spaceAfter || punctAfter),
      });
    }
  }
  const spans: Span[] = [];
  let opener: (typeof runs)[number] | null = null;
  for (const run of runs) {
    if (run.close && opener !== null) {
      spans.push({ from: opener.from, to: run.to, kind: "mark" });
      opener = null;
    } else if (run.open) opener = run;
  }
  return spans;
}

/** `^[` ... its balanced `]`, over the text children from the one it opens in (other children count as no text). */
function noteSpans(texts: readonly TextAt[], text: string): Span[] {
  const spans: Span[] = [];
  texts.forEach(({ from, source }, index) => {
    for (const match of source.matchAll(/\^\[/g)) {
      const start = from + match.index;
      if (escaped(text, start)) continue;
      let depth = 0;
      let end = -1;
      scan: for (const piece of texts.slice(index)) {
        const begin = piece === texts[index] ? start + 1 : piece.from;
        for (let at = begin; at < piece.to; at++) {
          const char = text[at];
          if (char === "\n") break scan;
          if (escaped(text, at)) continue;
          if (char === "[") depth += 1;
          else if (char === "]" && --depth === 0) {
            end = at + 1;
            break scan;
          }
        }
      }
      if (end > start + 3) spans.push({ from: start, to: end, kind: "note" });
    }
  });
  return spans;
}

/** Spans that do not overlap, the outermost first; each covers whole children but the text it starts | ends in. */
function kept(spans: Span[], children: readonly PhrasingContent[], text: string): Span[] {
  const sorted = spans.sort((a, b) => a.from - b.from || b.to - a.to);
  const out: Span[] = [];
  for (const span of sorted) {
    const last = out.at(-1);
    if (last !== undefined && span.from < last.to) continue;
    if (text.slice(span.from, span.to).includes("\n")) continue;
    const cuts = children.every((child) => {
      const at = offsetsOf(child);
      if (at === null) return false;
      const [from, to] = at;
      const apart = to <= span.from || from >= span.to;
      const inside = from >= span.from && to <= span.to;
      // a text cut in two must be its source as written (no escape | reference in it)
      return apart || inside || (child.type === "text" && text.slice(from, to) === child.value);
    });
    if (cuts) out.push(span);
  }
  return out;
}

function pointAt(text: string, offset: number): Point {
  const before = text.slice(0, offset);
  const lineStart = before.lastIndexOf("\n") + 1;
  return {
    line: (before.match(/\n/g)?.length ?? 0) + 1,
    column: offset - lineStart + 1,
    offset,
  };
}

function textPart(text: string, from: number, to: number): PhrasingContent {
  return {
    type: "text",
    value: text.slice(from, to),
    position: { start: pointAt(text, from), end: pointAt(text, to) },
  };
}

/** `children` with each span made its piece; the texts around a span keep their parts. */
function withPieces(
  children: readonly PhrasingContent[],
  spans: readonly Span[],
  text: string,
): PhrasingContent[] {
  const out: PhrasingContent[] = [];
  let open: Span | null = null;
  let next = 0;
  for (const child of children) {
    const [from, to] = offsetsOf(child) ?? [0, 0];
    let at = from;
    if (open !== null) {
      if (open.to >= to) continue;
      at = open.to;
      open = null;
    }
    while (next < spans.length && (spans[next]?.from ?? to) < to) {
      const span = spans[next];
      next += 1;
      if (span === undefined) break;
      if (span.from > at) out.push(textPart(text, at, span.from));
      out.push({
        type: "inlineSource",
        kind: span.kind,
        value: text.slice(span.from, span.to),
        position: { start: pointAt(text, span.from), end: pointAt(text, span.to) },
      });
      at = Math.min(span.to, to);
      if (span.to > to) {
        open = span;
        break;
      }
    }
    if (at < to) out.push(at === from ? child : textPart(text, at, to));
  }
  return out;
}

function piecesOf(children: readonly PhrasingContent[], text: string): PhrasingContent[] | null {
  const texts: TextAt[] = [];
  for (const child of children) {
    const at = offsetsOf(child);
    if (child.type === "text" && at !== null) {
      texts.push({ from: at[0], to: at[1], source: text.slice(at[0], at[1]) });
    }
  }
  if (!texts.some(({ source }) => /==|:|\^\[/.test(source))) return null;
  const spans = kept(
    [...markSpans(texts, text), ...emojiSpans(texts, text), ...noteSpans(texts, text)],
    children,
    text,
  );
  return spans.length === 0 ? null : withPieces(children, spans, text);
}

/**
 * Every marked text, shortcode emoji and inline note in a line of `tree` (read from `text`) becomes its piece; the
 * nodes read are the ones `htmlPieces` reads.
 */
export function syntaxPieces(tree: Root, text: string): void {
  const visit = (node: Nodes): void => {
    if (!("children" in node)) return;
    node.children.forEach(visit);
    switch (node.type) {
      case "paragraph":
      case "heading":
      case "tableCell":
      case "emphasis":
      case "strong":
      case "delete":
      case "link":
      case "linkReference": {
        const pieces = piecesOf(node.children, text);
        if (pieces !== null) node.children = pieces;
        break;
      }
      default:
        break;
    }
  };
  visit(tree);
}
