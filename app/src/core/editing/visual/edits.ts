// Whole-text edits in the Edit-mode editor (correction). Offsets are into the composed text (`Composed.text`, the
// session's edit text); a scope covers whole top-level blocks. An edit goes into its block's Markdown, the block is
// read again by the editor (`classify`: the same checks as at load) and its node replaced; every other node stays
// the same object, so `compose` keeps its bytes. Only the touched block is written by the editor (as typing would).

import { closeHistory } from "@milkdown/kit/prose/history";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import type { EditorState, Transaction } from "@milkdown/kit/prose/state";
import type { EditScope, TextEdit, TextRange } from "../session.svelte";
import { checkEdits } from "../textEdits";
import { blocksUsingReferences, classify } from "./classify";
import type { Composed } from "./compose";
import { LOCKED, type Core } from "./core";

/** Document index of the top-level node whose text holds `from..to` (`null`: between blocks | outside). */
export function blockOf(composed: Composed, from: number, to: number): number | null {
  const index = composed.ranges.findIndex((r) => r !== null && r.from <= from && to <= r.to);
  return index < 0 ? null : index;
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
 *  one is outside a block | across two, or a block is locked | would read as something else. All or nothing. */
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
  // the whole text once edited: a block may now use a reference defined elsewhere (written escaped: not editable)
  let edited = text;
  for (const edit of [...sorted].reverse()) {
    edited = edited.slice(0, edit.from) + edit.insert + edited.slice(edit.to);
  }
  const written = composed.ranges.flatMap((r, index) => (r === null ? [] : [index]));
  const usesReference = blocksUsingReferences(core, edited, startsAfter(composed, sorted));
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
    if (!verdict.editable || verdict.node.type !== node.type) return null;
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
