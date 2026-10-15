// Small DOM helpers shared by the document view, the selection popover and Follow.

import { parseSourcepos } from "../sourcepos";

/** True while the user has text selected (Follow must not yank the page away then). */
export function isTextSelected(): boolean {
  const selection = window.getSelection();
  return !!selection && !selection.isCollapsed && selection.toString().trim() !== "";
}

/** Text of a heading without the injected "#" anchor. */
export function headingText(heading: Element): string {
  const clone = heading.cloneNode(true) as Element;
  clone.querySelectorAll(".anchor").forEach((a) => a.remove());
  return (clone.textContent ?? "").replace(/^#/, "").trim();
}

const HEADING = /^H[1-6]$/;

/** The nearest heading above (or at) the block that contains `node`. */
export function sectionOf(node: Node, body: HTMLElement): string | null {
  let el: Element | null =
    node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
  while (el && el.parentElement !== body) el = el.parentElement;
  while (el) {
    if (HEADING.test(el.tagName)) return headingText(el);
    el = el.previousElementSibling;
  }
  return null;
}

/** What the app adds to the text of a document and the reader did not write: a heading's anchor, a code block's label. */
const DRAWN = ".anchor, .code-block__lang, .code-block__copy, .markdown-alert-title, script, style";
/** Elements whose end is a line break in the text a reader selects. */
const LINE_ENDS =
  "p, li, h1, h2, h3, h4, h5, h6, tr, pre, blockquote, dt, dd, figcaption, summary, div, br";
/** Stands where a code block was while the text around it is tidied (it is not text a document has). */
const CODE_MARK = "";

/**
 * The text of what `range` covers, as a reader sees it: without what the app drew around the document's text (a
 * heading's "#", a code block's label), with a line break where a block ends and a tab between cells. The spaces and
 * line breaks of a code block are kept as written when `keepCode` is set (a copied snippet keeps its indentation);
 * elsewhere a break stands for any run of spaces and breaks.
 */
export function readableText(range: Range, keepCode = false): string {
  const container = range.commonAncestorContainer;
  const host = container instanceof Element ? container : container.parentElement;
  // only what the app drew is selected (a heading's "#"): nothing the document says
  if (host?.closest(DRAWN)) return "";
  const holder = document.createElement("div");
  holder.append(range.cloneContents());
  holder.querySelectorAll(DRAWN).forEach((el) => el.remove());
  // a selection inside one code block holds only part of it: its text is all there is to read
  if (keepCode && host?.closest("pre")) return (holder.textContent ?? "").trim();
  const code: string[] = [];
  if (keepCode) {
    holder.querySelectorAll("pre").forEach((pre) => {
      code.push((pre.textContent ?? "").replace(/\n$/, ""));
      pre.textContent = `${CODE_MARK}${code.length - 1}${CODE_MARK}`;
    });
  }
  holder.querySelectorAll("td, th").forEach((cell) => cell.append("\t"));
  holder.querySelectorAll(LINE_ENDS).forEach((el) => el.append("\n"));
  const text = (holder.textContent ?? "").replace(/\s*\n\s*/g, "\n").trim();
  return text.replace(
    new RegExp(`${CODE_MARK}(\\d+)${CODE_MARK}`, "g"),
    (_, index: string) => code[Number(index)],
  );
}

export type SelectionInfo = { text: string; section: string | null; rect: DOMRect };

/** The selection if it lies inside `body`. Its text is the document's: the "#" of a heading's anchor is not in it. */
export function selectionIn(body: HTMLElement): SelectionInfo | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!body.contains(range.commonAncestorContainer)) return null;
  const text = readableText(range, true);
  if (text === "") return null;
  return {
    text,
    section: sectionOf(selection.anchorNode ?? range.startContainer, body),
    rect: range.getBoundingClientRect(),
  };
}

/** The top-level block of the rendered document that contains `node`. */
function blockOf(node: Node, body: HTMLElement): HTMLElement | null {
  let el: Element | null =
    node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
  while (el && el.parentElement !== body) el = el.parentElement;
  return el instanceof HTMLElement ? el : null;
}

/** The source lines of the blocks the selection starts and ends in; null when it is not inside blocks that know theirs. */
export function selectionLines(body: HTMLElement): { startLine: number; endLine: number } | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  const first = blockOf(range.startContainer, body);
  const last = blockOf(range.endContainer, body);
  const start = parseSourcepos(first?.getAttribute("data-sourcepos"));
  const end = parseSourcepos(last?.getAttribute("data-sourcepos"));
  return start && end ? { startLine: start.startLine, endLine: end.endLine } : null;
}

/** Finds an element by id inside `root` without colliding with the app's own ids. */
export function findById(root: HTMLElement, id: string): HTMLElement | null {
  const candidates = [id];
  try {
    const decoded = decodeURIComponent(id);
    if (decoded !== id) candidates.push(decoded);
  } catch {
    // not percent-encoded
  }
  for (const candidate of candidates) {
    const hit = root.querySelector<HTMLElement>(`[id="${CSS.escape(candidate)}"]`);
    if (hit) return hit;
  }
  return null;
}

/** The block-level element to scroll to for an anchor target (an `a.anchor` inside a heading, a footnote item, ...). */
export function anchorTarget(el: HTMLElement): HTMLElement {
  const heading = el.closest<HTMLElement>("h1, h2, h3, h4, h5, h6");
  return heading ?? el;
}

/** True inside a field the user types into: plain-letter shortcuts must leave those keys alone. */
export function isEditable(target: EventTarget | null): boolean {
  if (typeof Element === "undefined" || !(target instanceof Element)) return false;
  return (
    target.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])") !==
    null
  );
}
