// Comments in the margin of the page: a bar left of the blocks a comment touches (classes and attributes on the blocks,
// drawn by CSS), a count where several comments start in one block, and a click in the margin that shows the comment in
// the tab. The blocks are the page's own; this only marks them and takes the marks off again.

import type { CommentView } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import type { PageSelection } from "../../core/plugins/props";
import { touchedBlocks } from "./model";
import "./comments.css";

const MARK = "pm-cmt";
const FIRST = "pm-cmt-first";

/** Takes every mark off the page. */
export function clearMarks(root: HTMLElement): void {
  for (const el of root.querySelectorAll<HTMLElement>(`.${MARK}`)) {
    el.classList.remove(MARK, FIRST);
    el.removeAttribute("data-cmt");
    el.removeAttribute("data-cmt-count");
  }
}

/** Marks the blocks the comments touch (the bars of the document shown). */
export function applyMarks(root: HTMLElement, bars: readonly CommentView[]): void {
  clearMarks(root);
  const view = documents.view;
  if (view?.body.kind !== "markdown") return;
  const blocks = view.body.blocks;
  const ids = new Map<string, string[]>();
  const starts = new Map<string, number>();
  for (const bar of bars) {
    const touched = touchedBlocks(blocks, bar.line, bar.endLine);
    touched.forEach((id, at) => {
      ids.set(id, [...(ids.get(id) ?? []), bar.id]);
      if (at === 0) starts.set(id, (starts.get(id) ?? 0) + 1);
    });
  }
  for (const [id, comments] of ids) {
    const el = documents.host.element(id);
    if (!el) continue;
    el.classList.add(MARK);
    el.setAttribute("data-cmt", comments.join(" "));
    const first = starts.get(id) ?? 0;
    if (first > 0) {
      el.classList.add(FIRST);
      el.setAttribute("data-cmt-count", String(first));
    }
  }
}

/** The comment a click in the margin of a marked block means (the bar stands left of the text). */
export function commentAt(event: MouseEvent): string | null {
  const target = event.target;
  if (!(target instanceof Element)) return null;
  const el = target.closest<HTMLElement>(`.${MARK}`);
  if (!el || event.clientX >= el.getBoundingClientRect().left) return null;
  return el.getAttribute("data-cmt")?.split(" ")[0] ?? null;
}

/** The text selected in the page (not an input), with where it is; `null` when nothing is selected there. */
export function readSelection(): PageSelection | null {
  const container = documents.host.container;
  const selection = typeof window === "undefined" ? null : window.getSelection();
  if (!container || !selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!container.contains(range.commonAncestorContainer)) return null;
  const text = selection.toString();
  if (text.trim() === "") return null;
  return {
    text,
    node: selection.anchorNode,
    endNode: selection.focusNode,
    rect: rectOf(range),
  };
}

/** Where a range is on screen (an environment without layout has none). */
function rectOf(range: Range): DOMRect | null {
  return typeof range.getBoundingClientRect === "function" ? range.getBoundingClientRect() : null;
}
