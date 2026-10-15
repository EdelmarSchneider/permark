// Reads the reading view for the comments: the passage the person selected (as text, without what the app drew
// around it), the lines and headings it is under, and the highlight that keeps it visible while a comment is typed.

import { headingAtLine, headingPath } from "../agentContext";
import { selectionLines, sectionOf } from "./domUtil";
import { readHeadingLines } from "./outlineDom";

/** What the app adds to the text of a document and the reader did not write: a heading's anchor, a code block's label. */
const DRAWN = ".anchor, .code-block__lang, .code-block__copy, .markdown-alert-title, script, style";
/** Elements whose end is a line break in the text a reader selects. */
const LINE_ENDS =
  "p, li, h1, h2, h3, h4, h5, h6, tr, pre, blockquote, dt, dd, figcaption, summary, div, br";

/** The text of a selection: what the reader selected, with a line break where a block ends and a tab between cells. */
export function quoteOf(range: Range): string {
  const holder = document.createElement("div");
  holder.append(range.cloneContents());
  holder.querySelectorAll(DRAWN).forEach((el) => el.remove());
  holder.querySelectorAll("td, th").forEach((cell) => cell.append("\t"));
  holder.querySelectorAll(LINE_ENDS).forEach((el) => el.append("\n"));
  return (holder.textContent ?? "").replace(/\s*\n\s*/g, "\n").trim();
}

/** A selection in the reading view that a comment can be written on. */
export type CommentTarget = {
  /** The selected text. */
  quote: string;
  /** The lines of the blocks the selection touches (from 1); 0 when the view cannot tell. */
  startLine: number;
  endLine: number;
  /** The headings above the selection, its own section last. */
  headingPath: string[];
  /** Where the selection is on screen. */
  rect: DOMRect;
  /** The selected range, kept so the passage can be highlighted while the comment is typed. */
  range: Range;
};

/** The selection inside `body`, ready for a comment; `null` when nothing there is selected. */
export function commentTargetIn(body: HTMLElement): CommentTarget | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!body.contains(range.commonAncestorContainer)) return null;
  const quote = quoteOf(range);
  if (quote === "") return null;
  const lines = selectionLines(body);
  let path: string[];
  if (lines) {
    const { headings } = readHeadingLines(body);
    const at = headingAtLine(headings, lines.startLine);
    path = at === null ? [] : headingPath(headings, at);
  } else {
    const section = sectionOf(range.startContainer, body);
    path = section === null ? [] : [section];
  }
  return {
    quote,
    startLine: lines?.startLine ?? 0,
    endLine: lines?.endLine ?? 0,
    headingPath: path,
    rect: range.getBoundingClientRect(),
    range: range.cloneRange(),
  };
}

type Highlights = { set(name: string, value: unknown): void; delete(name: string): void };

/** Paints a range with the CSS Custom Highlight API (nothing happens where it is missing), or clears the highlight. */
export function highlight(name: string, range: Range | null): void {
  const registry =
    typeof CSS === "undefined"
      ? undefined
      : (CSS as unknown as { highlights?: Highlights }).highlights;
  if (!registry) return;
  const Ctor = (globalThis as unknown as { Highlight?: new (...r: Range[]) => unknown }).Highlight;
  if (range && Ctor) registry.set(name, new Ctor(range));
  else registry.delete(name);
}
