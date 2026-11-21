// Whole-text edits in the Edit-mode editor (correction). Offsets are into the composed text (`Composed.text`, the
// session's edit text); a scope covers whole top-level blocks. An edit goes into its block's Markdown, the block is
// read again by the editor (`classify`: the same checks as at load) and its node replaced; every other node stays
// the same object, so `compose` keeps its bytes. Only the touched block is written by the editor (as typing would).
// Text offsets -> document positions (`docRange`: reveal, correction marks): the range's text found in its block's
// shown text (markup left out) where the text around it matches best. An inline piece kept as written (a formula, a
// reference) is not prose: no range in its source is found, no edit may change it. Where an edit may go is one rule
// (`editableBlock`), asked before a run too (`changeable`): what could not be put in is not sent to a model.

import { closeHistory } from "@milkdown/kit/prose/history";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { TextSelection, type EditorState, type Transaction } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";
import type { EditScope, TextEdit, TextRange } from "../session.svelte";
import { checkEdits } from "../textEdits";
import { blocksUsingReferences, classify, mayUsePicture } from "./classify";
import type { Composed } from "./compose";
import { LOCKED, type Core } from "./core";
import { pieceSources } from "./inlineSource";

/** Document index of the top-level node whose text holds `from..to` (`null`: between blocks | outside). */
export function blockOf(composed: Composed, from: number, to: number): number | null {
  const index = composed.ranges.findIndex((r) => r !== null && r.from <= from && to <= r.to);
  return index < 0 ? null : index;
}

/** Document position of the top-level node holding `line` of the composed text: the last one starting at | above it
 *  (0: none does, the start). */
export function blockAtLine(doc: PMNode, composed: Composed, line: number): number {
  let found = 0;
  doc.forEach((node, offset) => {
    const at = composed.lines.get(node);
    if (at !== undefined && at <= line) found = offset;
  });
  return found;
}

/** A top-level node an edit may remake: its document index, the node and its range in the composed text. */
type EditableBlock = { index: number; node: PMNode; range: TextRange };

/** The block whose text holds `from..to` when an edit may remake it; `null`: between blocks | across two | outside,
 *  or a locked block. */
function editableBlock(
  doc: PMNode,
  composed: Composed,
  from: number,
  to: number,
): EditableBlock | null {
  const index = blockOf(composed, from, to);
  const range = index === null ? null : composed.ranges[index];
  const node = index === null ? null : doc.maybeChild(index);
  if (index === null || !range || !node || node.type.name === LOCKED) return null;
  return { index, node, range };
}

/** Context chars compared on each side of a candidate (`docRange`). */
const CONTEXT = 24;
/** Markup chars left out of both sides before contexts are compared (an escaped one is shown as itself). */
const MARKUP = /[\\*_~`[\]()#>|]/g;
/** `MARKUP` by char code (all ASCII), for the comparisons made once per candidate. */
const IS_MARKUP = new Uint8Array(128);
for (const c of "\\*_~`[]()#>|") IS_MARKUP[c.charCodeAt(0)] = 1;

/** A block's shown text and the document position of each of its chars. */
type Shown = { text: string; at: number[] };

/** A block's shown text (its text nodes; `\n` between textblocks and for inline leaves) and each char's position. */
function shownText(node: PMNode, start: number): Shown {
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

/** Chars `context` (markup left out) ends with that the shown chars just before `at` (the `CONTEXT` before it,
 *  markup left out) end with too. Walks the shown text in place: no copy per candidate. */
function commonTail(context: string, text: string, at: number): number {
  let n = 0;
  for (let i = at - 1; i >= Math.max(0, at - CONTEXT) && n < context.length; i--) {
    const c = text.charCodeAt(i);
    if (IS_MARKUP[c] === 1) continue;
    if (c !== context.charCodeAt(context.length - 1 - n)) break;
    n += 1;
  }
  return n;
}

/** Chars `context` (markup left out) starts with that the shown chars from `at` on (the `CONTEXT` from there, markup
 *  left out) start with too. */
function commonHead(context: string, text: string, at: number): number {
  let n = 0;
  const end = Math.min(text.length, at + CONTEXT);
  for (let i = at; i < end && n < context.length; i++) {
    const c = text.charCodeAt(i);
    if (IS_MARKUP[c] === 1) continue;
    if (c !== context.charCodeAt(n)) break;
    n += 1;
  }
  return n;
}

/** Where `range` (in the block at document index `index`, written at `block`, starting at document position `start`,
 *  shown as `shown`) is shown: see `docRange`. Candidates are tried nearest the expected spot first (ties: the
 *  earlier one), so the first with the best score wins and a candidate matching both contexts in full ends the
 *  search: the same place as trying every one in text order. */
function placeIn(
  composed: Composed,
  index: number,
  block: TextRange,
  start: number,
  shown: Shown,
  range: TextRange,
): TextRange | null {
  // where the text's own reading has the pieces: a prose copy of a piece's source is prose
  const pieces = composed.pieces[index] ?? [];
  if (pieces.some((span) => range.from < span.to && range.to > span.from)) return null;
  const markdown = composed.text.slice(block.from, block.to);
  const rel = range.from - block.from;
  const needle = composed.text.slice(range.from, range.to);
  const before = markdown.slice(Math.max(0, rel - CONTEXT), rel).replace(MARKUP, "");
  const after = markdown
    .slice(rel + needle.length, rel + needle.length + CONTEXT)
    .replace(MARKUP, "");
  const text = shown.text;
  const expected = (rel * text.length) / Math.max(1, markdown.length);
  // every place the needle starts at (an insertion point: every place of the text, end included)
  const places: number[] = [];
  if (needle !== "") {
    for (let at = text.indexOf(needle); at >= 0; at = text.indexOf(needle, at + 1)) places.push(at);
  }
  const count = needle === "" ? text.length + 1 : places.length;
  const placeAt = (i: number): number => (needle === "" ? i : (places[i] ?? 0));
  // the first place at | after the expected spot, and the last one before it
  let lo = 0;
  let hi = count;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (placeAt(mid) < expected) lo = mid + 1;
    else hi = mid;
  }
  let right = lo;
  let left = lo - 1;
  const full = before.length + after.length;
  let best: { at: number; score: number } | null = null;
  while (left >= 0 || right < count) {
    const offLeft = left >= 0 ? Math.abs(placeAt(left) - expected) : Infinity;
    const offRight = right < count ? Math.abs(placeAt(right) - expected) : Infinity;
    const at = offLeft <= offRight ? placeAt(left--) : placeAt(right++);
    const score = commonTail(before, text, at) + commonHead(after, text, at + needle.length);
    if (best === null || score > best.score) best = { at, score };
    if (score === full) break;
  }
  if (best === null) return null;
  const from = shown.at[best.at] ?? (shown.at[best.at - 1] ?? start) + 1;
  if (needle === "") return { from, to: from };
  const last = shown.at[best.at + needle.length - 1];
  return last === undefined ? null : { from, to: last + 1 };
}

/** Index in `written` (document indexes of the blocks with a range, in order) of the first block whose range holds
 *  `from..to`, as a scan in document order finds it (the ranges follow each other without overlap); `null`: none. */
function writtenHolding(
  composed: Composed,
  written: readonly number[],
  from: number,
  to: number,
): number | null {
  let lo = 0;
  let hi = written.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((composed.ranges[written[mid] ?? 0]?.to ?? 0) < to) lo = mid + 1;
    else hi = mid;
  }
  const index = written[lo];
  const range = index === undefined ? null : composed.ranges[index];
  return index !== undefined && range && range.from <= from ? index : null;
}

/** Document positions of each of `ranges` (offsets of `composed.text`): see `docRange`. Each block is looked up by
 *  binary search and its shown text made once, however many ranges it holds (a document's marks in one pass). */
export function docRanges(
  doc: PMNode,
  composed: Composed,
  ranges: readonly TextRange[],
): (TextRange | null)[] {
  const starts: number[] = [];
  doc.forEach((_, offset) => starts.push(offset));
  const written: number[] = [];
  composed.ranges.forEach((range, index) => {
    if (range !== null) written.push(index);
  });
  const shown = new Map<number, Shown>();
  return ranges.map((range) => {
    const index = writtenHolding(composed, written, range.from, range.to);
    const block = index === null ? null : composed.ranges[index];
    const node = index === null ? null : doc.maybeChild(index);
    if (index === null || !block || !node || node.type.name === LOCKED) return null;
    const start = starts[index] ?? 0;
    let text = shown.get(index);
    if (text === undefined) {
      text = shownText(node, start);
      shown.set(index, text);
    }
    return placeIn(composed, index, block, start, text, range);
  });
}

/** Document positions of `range` (offsets of `composed.text`): its text in the shown text of its block, at the place
 *  whose text before | after matches the Markdown around the range best (ties: nearest the expected spot, then the
 *  earlier one). An empty range: an insertion point. `null`: outside | across blocks, a locked block, inside an inline
 *  piece's source, or not shown as such (markup inside). Many ranges at once: `docRanges`. */
export function docRange(doc: PMNode, composed: Composed, range: TextRange): TextRange | null {
  return docRanges(doc, composed, [range])[0] ?? null;
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
    // a locked block too: what an edit may change in a scope is another question (`changeable`)
    case "paragraph": {
      const index = $from.index(0);
      return state.doc.maybeChild(index) ? span(composed, index, index) : null;
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
  const byBlock = new Map<number, { block: EditableBlock; edits: TextEdit[] }>();
  for (const edit of sorted) {
    const block = editableBlock(state.doc, composed, edit.from, edit.to);
    if (block === null) return null;
    const held = byBlock.get(block.index);
    if (held) held.edits.push(edit);
    else byBlock.set(block.index, { block, edits: [edit] });
  }
  // the whole text once edited: a block may now use a picture by reference (written escaped: not editable)
  let edited = text;
  for (const edit of [...sorted].reverse()) {
    edited = edited.slice(0, edit.from) + edit.insert + edited.slice(edit.to);
  }
  const written = composed.ranges.flatMap((r, index) => (r === null ? [] : [index]));
  const starts = startsAfter(composed, sorted);
  // read as a whole (long in a large document with a definition) only when a touched block could use one
  const usesReference = [...byBlock.keys()].some((index) =>
    mayUsePicture(edited, starts, written.indexOf(index)),
  )
    ? blocksUsingReferences(core, edited, starts).blocks
    : new Set<number>();
  const at: number[] = [];
  state.doc.forEach((_, offset) => at.push(offset));
  const replaced: { from: number; to: number; node: PMNode }[] = [];
  for (const [index, { block, edits: blockEdits }] of byBlock) {
    const { node, range } = block;
    const pos = at[index];
    if (pos === undefined) return null;
    let markdown = text.slice(range.from, range.to);
    for (const edit of [...blockEdits].reverse()) {
      const from = edit.from - range.from;
      markdown = markdown.slice(0, from) + edit.insert + markdown.slice(edit.to - range.from);
    }
    const verdict = classify(core, markdown, null, usesReference.has(written.indexOf(index)));
    // a fix never changes a formula (its TeX is no prose)
    const kept = JSON.stringify(pieceSources(node));
    if (
      !verdict.editable ||
      verdict.node.type !== node.type ||
      JSON.stringify(pieceSources(verdict.node)) !== kept
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

/** An edit of `range` would go in as the document stands: the range lies in one block the editor writes itself
 *  (`editableBlock`); with `insert`, that very edit passes every check of `editsTr`. Nothing is changed. */
export function changeable(
  core: Core,
  composed: Composed,
  range: TextRange,
  insert?: string,
): boolean {
  const state = core.view.state;
  if (insert === undefined) {
    return editableBlock(state.doc, composed, range.from, range.to) !== null;
  }
  const expect = composed.text.slice(range.from, range.to);
  const edit: TextEdit = { from: range.from, to: range.to, insert, expect };
  return editsTr(core, state, composed, [edit]) !== null;
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
