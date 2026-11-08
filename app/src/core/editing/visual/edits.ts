// Whole-text edits in the Edit-mode editor (correction). Offsets are into the composed text (`Composed.text`, the
// session's edit text); a scope covers whole top-level blocks. An edit goes into its block's Markdown, the block is
// read again by the editor (`classify`: the same checks as at load) and its node replaced; every other node stays
// the same object, so `compose` keeps its bytes. Only the touched block is written by the editor (as typing would).
// Text offsets -> document positions (`docRange`: reveal, correction marks): the range's text found in its block's
// shown text (markup left out) where the text around it matches best. An inline piece kept as written (a formula, a
// reference) is not prose: no range in its source is found, no edit may change it.

import { closeHistory } from "@milkdown/kit/prose/history";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { TextSelection, type EditorState, type Transaction } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";
import type { EditScope, TextEdit, TextRange } from "../session.svelte";
import { checkEdits } from "../textEdits";
import { blocksUsingReferences, classify } from "./classify";
import type { Composed } from "./compose";
import { INLINE_SOURCE, LOCKED, type Core } from "./core";

/** Document index of the top-level node whose text holds `from..to` (`null`: between blocks | outside). */
export function blockOf(composed: Composed, from: number, to: number): number | null {
  const index = composed.ranges.findIndex((r) => r !== null && r.from <= from && to <= r.to);
  return index < 0 ? null : index;
}

/** Context chars compared on each side of a candidate (`docRange`). */
const CONTEXT = 24;
/** Markup chars left out of both sides before contexts are compared (an escaped one is shown as itself). */
const MARKUP = /[\\*_~`[\]()#>|]/g;

/** A block's shown text (its text nodes; `\n` between textblocks and for inline leaves) and each char's position. */
function shownText(node: PMNode, start: number): { text: string; at: number[] } {
  let text = "";
  const at: number[] = [];
  node.descendants((child, pos) => {
    const here = start + 1 + pos;
    if (child.isText) {
      const own = child.text ?? "";
      for (let i = 0; i < own.length; i++) at.push(here + i);
      text += own;
      return false;
    }
    if (child.isLeaf || (child.isTextblock && text !== "")) {
      text += "\n";
      at.push(here);
    }
    return true;
  });
  return { text, at };
}

/** The sources of the inline pieces in `node`, in order. */
function pieces(node: PMNode): string[] {
  const found: string[] = [];
  node.descendants((child) => {
    if (child.type.name === INLINE_SOURCE) found.push(String(child.attrs.source));
    return true;
  });
  return found;
}

/** Offsets of the inline pieces of `node` in `markdown`, its text (each written as its source, in order). */
function pieceSpans(node: PMNode, markdown: string): TextRange[] {
  const spans: TextRange[] = [];
  let next = 0;
  for (const source of pieces(node)) {
    const at = markdown.indexOf(source, next);
    if (at < 0) continue;
    spans.push({ from: at, to: at + source.length });
    next = at + source.length;
  }
  return spans;
}

function commonTail(markdown: string, shown: string): number {
  const a = markdown.replace(MARKUP, "");
  const b = shown.replace(MARKUP, "");
  let n = 0;
  while (n < a.length && n < b.length && a[a.length - 1 - n] === b[b.length - 1 - n]) n += 1;
  return n;
}

function commonHead(markdown: string, shown: string): number {
  const a = markdown.replace(MARKUP, "");
  const b = shown.replace(MARKUP, "");
  let n = 0;
  while (n < a.length && n < b.length && a[n] === b[n]) n += 1;
  return n;
}

/** Document positions of `range` (offsets of `composed.text`): its text in the shown text of its block, at the place
 *  whose text before | after matches the Markdown around the range best (ties: nearest the expected spot). An empty
 *  range: an insertion point. `null`: outside | across blocks, a locked block, inside an inline piece's source, or not
 *  shown as such (markup inside). */
export function docRange(doc: PMNode, composed: Composed, range: TextRange): TextRange | null {
  const index = blockOf(composed, range.from, range.to);
  const block = index === null ? null : composed.ranges[index];
  const node = index === null ? null : doc.maybeChild(index);
  if (index === null || !block || !node || node.type.name === LOCKED) return null;
  let start = 0;
  for (let i = 0; i < index; i++) start += doc.child(i).nodeSize;
  const shown = shownText(node, start);
  const markdown = composed.text.slice(block.from, block.to);
  const rel = range.from - block.from;
  const end = range.to - block.from;
  if (pieceSpans(node, markdown).some((span) => rel < span.to && end > span.from)) return null;
  const needle = composed.text.slice(range.from, range.to);
  const before = markdown.slice(Math.max(0, rel - CONTEXT), rel);
  const after = markdown.slice(rel + needle.length, rel + needle.length + CONTEXT);
  const expected = (rel * shown.text.length) / Math.max(1, markdown.length);
  let best: { at: number; score: number; off: number } | null = null;
  for (let at = shown.text.indexOf(needle); at >= 0; at = shown.text.indexOf(needle, at + 1)) {
    const score =
      commonTail(before, shown.text.slice(Math.max(0, at - CONTEXT), at)) +
      commonHead(after, shown.text.slice(at + needle.length, at + needle.length + CONTEXT));
    const off = Math.abs(at - expected);
    if (best === null || score > best.score || (score === best.score && off < best.off)) {
      best = { at, score, off };
    }
    if (at >= shown.text.length) break;
  }
  if (best === null) return null;
  const from = shown.at[best.at] ?? (shown.at[best.at - 1] ?? start) + 1;
  if (needle === "") return { from, to: from };
  const last = shown.at[best.at + needle.length - 1];
  return last === undefined ? null : { from, to: last + 1 };
}

/** Selects `range` (see `docRange`) and scrolls it into view; `false` (nothing done) when it is not found. */
export function selectRange(view: EditorView, composed: Composed, range: TextRange): boolean {
  const found = docRange(view.state.doc, composed, range);
  if (found === null) return false;
  const selection = TextSelection.create(view.state.doc, found.from, found.to);
  view.dispatch(view.state.tr.setSelection(selection).scrollIntoView());
  return true;
}

/** From the first written block of `first..last` (document indexes) to the end of the last one. */
function span(composed: Composed, first: number, last: number): TextRange | null {
  const written = composed.ranges.slice(first, last + 1).filter((r) => r !== null);
  const start = written[0];
  const end = written[written.length - 1];
  return start && end ? { from: start.from, to: end.to } : null;
}

export function scopeIn(state: EditorState, composed: Composed, kind: EditScope): TextRange | null {
  const { empty, $from, $to } = state.selection;
  switch (kind) {
    case "selection": {
      if (empty) return null;
      // a selection ending between two blocks (select all) does not take the block after
      const last = $to.depth === 0 ? $to.index(0) - 1 : $to.index(0);
      return span(composed, $from.index(0), last);
    }
    case "paragraph": {
      const index = $from.index(0);
      const node = state.doc.maybeChild(index);
      return node && node.type.name !== LOCKED ? span(composed, index, index) : null;
    }
    case "document":
      return { from: 0, to: composed.text.length };
    default:
      return kind satisfies never;
  }
}

/** Starts of the written blocks once `edits` (checked, in order) are in. */
function startsAfter(composed: Composed, edits: readonly TextEdit[]): number[] {
  const starts: number[] = [];
  let shift = 0;
  let next = 0;
  for (const range of composed.ranges) {
    if (range === null) continue;
    for (let edit = edits[next]; edit && edit.to < range.from; edit = edits[++next]) {
      shift += edit.insert.length - (edit.to - edit.from);
    }
    starts.push(range.from + shift);
  }
  return starts;
}

/** One transaction (its own undo step) remaking each block `edits` touch; `null` when they do not pass `checkEdits`,
 *  one is outside a block | across two, or a block is locked | would read as something else | would change an inline
 *  piece. All or nothing. */
export function editsTr(
  core: Core,
  state: EditorState,
  composed: Composed,
  edits: readonly TextEdit[],
): Transaction | null {
  const text = composed.text;
  const sorted = checkEdits(edits, text.length, (from, to) => text.slice(from, to));
  if (sorted === null) return null;
  const byBlock = new Map<number, TextEdit[]>();
  for (const edit of sorted) {
    const index = blockOf(composed, edit.from, edit.to);
    if (index === null) return null;
    byBlock.set(index, [...(byBlock.get(index) ?? []), edit]);
  }
  // the whole text once edited: a block may now use a picture by reference (written escaped: not editable)
  let edited = text;
  for (const edit of [...sorted].reverse()) {
    edited = edited.slice(0, edit.from) + edit.insert + edited.slice(edit.to);
  }
  const written = composed.ranges.flatMap((r, index) => (r === null ? [] : [index]));
  const usesReference = blocksUsingReferences(core, edited, startsAfter(composed, sorted)).blocks;
  const at: number[] = [];
  state.doc.forEach((_, offset) => at.push(offset));
  const replaced: { from: number; to: number; node: PMNode }[] = [];
  for (const [index, blockEdits] of byBlock) {
    const node = state.doc.maybeChild(index);
    const range = composed.ranges[index];
    const pos = at[index];
    if (!node || !range || pos === undefined || node.type.name === LOCKED) return null;
    let markdown = text.slice(range.from, range.to);
    for (const edit of [...blockEdits].reverse()) {
      const from = edit.from - range.from;
      markdown = markdown.slice(0, from) + edit.insert + markdown.slice(edit.to - range.from);
    }
    const verdict = classify(core, markdown, null, usesReference.has(written.indexOf(index)));
    // a fix never changes a formula (its TeX is no prose)
    const kept = JSON.stringify(pieces(node));
    if (
      !verdict.editable ||
      verdict.node.type !== node.type ||
      JSON.stringify(pieces(verdict.node)) !== kept
    ) {
      return null;
    }
    replaced.push({ from: pos, to: pos + node.nodeSize, node: verdict.node });
  }
  const tr = state.tr;
  // from the last block back: positions before a replaced block stay
  for (const { from, to, node } of replaced.sort((a, b) => b.from - a.from)) {
    tr.replaceWith(from, to, node);
  }
  return closeHistory(tr);
}

/** Puts `edits` in (see `editsTr`) as one undo step of its own: typing right after starts a new one. */
export function applyEdits(core: Core, composed: Composed, edits: readonly TextEdit[]): boolean {
  const view = core.view;
  const tr = editsTr(core, view.state, composed, edits);
  if (tr === null) return false;
  view.dispatch(tr);
  view.dispatch(closeHistory(view.state.tr));
  return true;
}
