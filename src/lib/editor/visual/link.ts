// Links in Visual mode: reading the link under the selection for the popover (Ctrl+K), and applying or removing it.

import type { MarkType } from "@milkdown/kit/prose/model";
import { TextSelection, type EditorState } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";

/** The link under the selection, or the selected text that would become one. */
export type LinkTarget = {
  from: number;
  to: number;
  text: string;
  href: string;
  /** The range is already a link. */
  existing: boolean;
};

function linkType(state: EditorState): MarkType {
  return state.schema.marks.link;
}

/** The range of the link that covers `pos`: every neighbouring piece of text with the same target (bold inside a link splits it). */
function markRange(
  state: EditorState,
  pos: number,
): { from: number; to: number; href: string } | null {
  const type = linkType(state);
  const $pos = state.doc.resolve(pos);
  const start = $pos.start();
  const pieces: { from: number; to: number; href: string | null }[] = [];
  $pos.parent.forEach((child, offset) => {
    const mark = type.isInSet(child.marks);
    const from = start + offset;
    pieces.push({
      from,
      to: from + child.nodeSize,
      href: mark ? String(mark.attrs.href ?? "") : null,
    });
  });
  const at = pieces.findIndex((p) => p.href !== null && pos >= p.from && pos <= p.to);
  if (at < 0) return null;
  const href = pieces[at].href as string;
  let first = at;
  let last = at;
  while (first > 0 && pieces[first - 1].href === href) first--;
  while (last < pieces.length - 1 && pieces[last + 1].href === href) last++;
  return { from: pieces[first].from, to: pieces[last].to, href };
}

/** What the link popover starts with: the link the caret is in, else the selected text, else nothing. */
export function linkTarget(state: EditorState): LinkTarget {
  const { from, to, empty } = state.selection;
  const range =
    markRange(state, empty ? from : from + 1) ?? (empty ? null : markRange(state, to - 1));
  if (range) {
    return {
      ...range,
      text: state.doc.textBetween(range.from, range.to),
      existing: true,
    };
  }
  return {
    from,
    to,
    text: empty ? "" : state.doc.textBetween(from, to),
    href: "",
    existing: false,
  };
}

/**
 * Applies the popover: a target makes the range a link (writing the text again when it changed, or when there was none), an
 * empty target takes the link off. Resolves to whether the document changed.
 */
export function applyLink(
  view: EditorView,
  target: Pick<LinkTarget, "from" | "to" | "text">,
  next: { text: string; href: string },
): boolean {
  const { state } = view;
  const type = linkType(state);
  const href = next.href.trim();
  const tr = state.tr;
  const currentText = state.doc.textBetween(target.from, target.to);
  if (href === "") {
    tr.removeMark(target.from, target.to, type);
  } else {
    const text = next.text === "" ? href : next.text;
    const mark = type.create({ href, title: null });
    if (text === currentText && target.from !== target.to) {
      tr.removeMark(target.from, target.to, type).addMark(target.from, target.to, mark);
    } else {
      tr.replaceWith(target.from, target.to, state.schema.text(text, [mark]));
      tr.setSelection(TextSelection.create(tr.doc, target.from + text.length));
      tr.removeStoredMark(type);
    }
  }
  if (!tr.docChanged) return false;
  view.dispatch(tr.scrollIntoView());
  view.focus();
  return true;
}
