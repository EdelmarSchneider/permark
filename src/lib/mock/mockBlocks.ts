// A stand-in for the backend's `parse_blocks`, for the browser mock and for tests of the block model: the
// top-level blocks of a Markdown text as UTF-8 byte ranges. It follows the same rules as the Rust side (a
// block is a run of whole lines, link reference definitions stay in the separators) but it is a line-based
// approximation of CommonMark, not comrak. Production ranges always come from Rust.

import type { BlockInfo, BlockKind, Eol, ParsedBlocks } from "../types";

type Line = { start: number; end: number; text: string };

const BLANK = /^\s*$/;
const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^`]*)$/;
const ATX = /^ {0,3}#{1,6}(?:[ \t]|$)/;
const HR = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const BULLET = /^ {0,3}(?:[-+*]|\d{1,9}[.)])(?:[ \t]|$)/;
const QUOTE = /^ {0,3}>/;
const HTML = /^ {0,3}<(?:[a-zA-Z][\w-]*|\/[a-zA-Z][\w-]*|!--|\?|![A-Z])/;
const TABLE_DELIM = /^ {0,3}\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$/;
const REFDEF = /^ {0,3}\[(?!\^)[^\]]+\]:[ \t]*\S/;
const FOOTNOTE = /^ {0,3}\[\^[^\]]+\]:/;
const SETEXT = /^ {0,3}(?:=+|-+)[ \t]*$/;
const MATH_OPEN = /^ {0,3}\$\$/;
const ALERT = /^ {0,3}>\s*\[!\w+\]/;

function splitLines(text: string, from: number): { lines: Line[]; eol: Eol } {
  const lines: Line[] = [];
  let lf = 0;
  let crlf = 0;
  let cr = 0;
  let start = from;
  let i = from;
  while (i < text.length) {
    const c = text.charCodeAt(i);
    if (c !== 10 && c !== 13) {
      i++;
      continue;
    }
    lines.push({ start, end: i, text: text.slice(start, i) });
    if (c === 13 && text.charCodeAt(i + 1) === 10) {
      crlf++;
      i += 2;
    } else {
      if (c === 10) lf++;
      else cr++;
      i++;
    }
    start = i;
  }
  if (start < text.length) lines.push({ start, end: text.length, text: text.slice(start) });
  const eol: Eol = cr === 0 && (lf === 0 || crlf === 0) ? (crlf > 0 ? "crlf" : "lf") : "mixed";
  return { lines, eol };
}

type Span = { first: number; last: number; kind: BlockKind };

/** The index of the last line of a block that runs until a blank line. */
function untilBlank(lines: Line[], from: number): number {
  let j = from;
  while (j + 1 < lines.length && !BLANK.test(lines[j + 1].text)) j++;
  return j;
}

function nextNonBlank(lines: Line[], from: number): number {
  let k = from;
  while (k < lines.length && BLANK.test(lines[k].text)) k++;
  return k;
}

function interrupts(text: string): boolean {
  return ATX.test(text) || FENCE.test(text) || HR.test(text) || QUOTE.test(text);
}

function spans(lines: Line[]): Span[] {
  const out: Span[] = [];
  const n = lines.length;
  let i = 0;
  if (n > 1 && lines[0].text === "---") {
    const close = lines.findIndex((l, k) => k > 0 && (l.text === "---" || l.text === "..."));
    if (close > 0) {
      out.push({ first: 0, last: close, kind: "frontmatter" });
      i = close + 1;
    }
  }
  while (i < n) {
    const text = lines[i].text;
    if (BLANK.test(text)) {
      i++;
      continue;
    }
    const fence = FENCE.exec(text);
    if (fence) {
      const closing = new RegExp(`^ {0,3}${fence[1][0]}{${fence[1].length},}\\s*$`);
      let j = i + 1;
      while (j < n && !closing.test(lines[j].text)) j++;
      const last = j < n ? j : n - 1;
      const kind: BlockKind = fence[2].trim().split(/\s+/)[0] === "math" ? "math" : "code";
      out.push({ first: i, last, kind });
      i = last + 1;
      continue;
    }
    if (ATX.test(text)) {
      out.push({ first: i, last: i, kind: "heading" });
      i++;
      continue;
    }
    if (HR.test(text)) {
      out.push({ first: i, last: i, kind: "thematicBreak" });
      i++;
      continue;
    }
    if (QUOTE.test(text)) {
      const last = untilBlank(lines, i);
      out.push({ first: i, last, kind: ALERT.test(text) ? "other" : "blockquote" });
      i = last + 1;
      continue;
    }
    if (HTML.test(text)) {
      const last = untilBlank(lines, i);
      out.push({ first: i, last, kind: "html" });
      i = last + 1;
      continue;
    }
    if (FOOTNOTE.test(text)) {
      let last = i;
      for (;;) {
        const next = last + 1;
        if (next < n && !BLANK.test(lines[next].text)) last = next;
        else if (next < n && /^ {4}/.test(lines[nextNonBlank(lines, next)]?.text ?? "")) {
          last = nextNonBlank(lines, next);
        } else break;
      }
      out.push({ first: i, last, kind: "footnoteDef" });
      i = last + 1;
      continue;
    }
    if (REFDEF.test(text)) {
      i++; // consumed by the parser: it stays in the separators
      continue;
    }
    if (BULLET.test(text)) {
      let last = i;
      for (;;) {
        const next = last + 1;
        if (next >= n) break;
        if (BLANK.test(lines[next].text)) {
          const k = nextNonBlank(lines, next);
          if (k < n && (/^ {2,}\S/.test(lines[k].text) || BULLET.test(lines[k].text))) last = k;
          else break;
        } else if (interrupts(lines[next].text) && !BULLET.test(lines[next].text)) break;
        else last = next;
      }
      out.push({ first: i, last, kind: "list" });
      i = last + 1;
      continue;
    }
    if (MATH_OPEN.test(text)) {
      const sameLine = /\$\$\s*$/.test(text) && text.trim().length > 2;
      let last = i;
      if (!sameLine) {
        last = i + 1;
        while (last < n && !lines[last].text.includes("$$")) last++;
        last = Math.min(last, n - 1);
      }
      out.push({ first: i, last, kind: "math" });
      i = last + 1;
      continue;
    }
    if (text.includes("|") && i + 1 < n && text.length > 0 && TABLE_DELIM.test(lines[i + 1].text)) {
      const last = untilBlank(lines, i);
      out.push({ first: i, last, kind: "table" });
      i = last + 1;
      continue;
    }
    // a paragraph, or a heading underlined with === or ---
    let last = i;
    let kind: BlockKind = "paragraph";
    while (last + 1 < n && !BLANK.test(lines[last + 1].text)) {
      const next = lines[last + 1].text;
      if (SETEXT.test(next)) {
        last++;
        kind = "heading";
        break;
      }
      if (interrupts(next) || (BULLET.test(next) && !/^\s*\d{2,}/.test(next))) break;
      last++;
    }
    out.push({ first: i, last, kind });
    i = last + 1;
  }
  return out;
}

/** The blocks of `text` (any line ending, an optional BOM) as UTF-8 byte ranges, like `parse_blocks`. */
export function mockParseBlocks(text: string): ParsedBlocks {
  const bom = text.startsWith("﻿") ? 1 : 0;
  const { lines, eol } = splitLines(text, bom);
  const encoder = new TextEncoder();
  const bytes = (s: string): number => encoder.encode(s).length;

  // byte offsets of the line starts and ends, walking the text once
  const starts: number[] = [];
  const ends: number[] = [];
  let char = 0;
  let byte = 0;
  for (const line of lines) {
    byte += bytes(text.slice(char, line.start));
    starts.push(byte);
    byte += bytes(line.text);
    ends.push(byte);
    char = line.end;
  }

  const blocks: BlockInfo[] = [];
  for (const span of spans(lines)) {
    let last = span.last;
    while (last > span.first && BLANK.test(lines[last].text)) last--;
    blocks.push({ startByte: starts[span.first], endByte: ends[last], kind: span.kind });
  }
  return { eol, blocks };
}
