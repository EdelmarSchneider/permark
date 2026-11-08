// The Edit-mode document and the text a save makes of it (spec: "Composing the Edit-mode text").
//
// `buildDoc` turns the blocks of a text (backend block map) into one top-level node each: an editable node from the
// block's own text, or a locked atom. ProseMirror keeps an unchanged node as the same object across transactions,
// so `compose` knows which nodes are untouched without tracking: an untouched block is written with its own bytes,
// the blank lines between two untouched neighbours too. An edited block that stands for one block keeps that block's
// bytes with only the editor's change spliced in (`splice`); anything else is written by the editor.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import type { Segment } from "../../../api/generated/Segment";
import { diff } from "../lines";
import type { TextRange } from "../session.svelte";
import { blocksUsingReferences, classify, sameMeaning } from "./classify";
import { LOCKED, type Core, type LockReason } from "./core";

/** One block of the text the document was made from. */
export type Source = {
  text: string;
  /** Offsets of the block in the text. */
  start: number;
  end: number;
  lineStart: number;
  kind: string;
  /** Node type name it became (`pm_locked` when locked). */
  type: string;
  locked: LockReason | null;
  /** The editor's writing of it at load (editable blocks). */
  written: string | null;
};

export type Built = {
  text: string;
  sources: Source[];
  /** `separators[i]` = text before block i; `separators[n]` = after the last. */
  separators: string[];
  /** Node object -> block index (the nodes made at load). */
  origin: WeakMap<PMNode, number>;
  doc: PMNode;
};

/** Offsets of each line start (index = line - 1). */
function lineStarts(text: string): number[] {
  const starts = [0];
  for (let i = 0; i < text.length; i++) if (text.charCodeAt(i) === 10) starts.push(i + 1);
  return starts;
}

/** Time-sliced work: yields to the page every few milliseconds (a long document never freezes it). */
async function sliced<T>(
  items: readonly T[],
  each: (item: T, index: number) => void,
): Promise<void> {
  let started = performance.now();
  for (let i = 0; i < items.length; i++) {
    each(items[i] as T, i);
    if (performance.now() - started > 8) {
      await new Promise((resolve) => setTimeout(resolve, 0));
      started = performance.now();
    }
  }
}

/** Builds the document for `text` with its block map. Composing it untouched gives `text` back. */
export async function buildDoc(
  core: Core,
  text: string,
  segments: readonly Segment[],
): Promise<Built> {
  const starts = lineStarts(text);
  const schema = core.view.state.schema;
  const lockedType = schema.nodes[LOCKED];
  if (!lockedType) throw new Error("no locked node");
  const sources: Source[] = [];
  const separators: string[] = [];
  let cursor = 0;
  for (const segment of segments) {
    const start = starts[segment.lineStart - 1] ?? text.length;
    const nextLine = starts[segment.lineEnd];
    const end = nextLine === undefined ? text.length : nextLine - 1;
    separators.push(text.slice(cursor, start));
    sources.push({
      text: text.slice(start, end),
      start,
      end,
      lineStart: segment.lineStart,
      kind: segment.kind,
      type: LOCKED,
      locked: segment.lock,
      written: null,
    });
    cursor = end;
  }
  separators.push(text.slice(cursor));
  const usesReference = blocksUsingReferences(
    core,
    text,
    sources.map((s) => s.start),
  );
  const nodes: PMNode[] = [];
  const origin = new WeakMap<PMNode, number>();
  await sliced(sources, (source, i) => {
    const verdict = classify(core, source.text, source.locked, usesReference.has(i));
    let node: PMNode;
    if (verdict.editable) {
      node = verdict.node;
      source.type = node.type.name;
      source.written = core.write(node);
    } else {
      source.locked = verdict.reason;
      node = lockedType.create({ source: source.text, kind: source.kind, reason: verdict.reason });
    }
    origin.set(node, i);
    nodes.push(node);
  });
  const doc = schema.topNodeType.create(
    null,
    nodes.length > 0 ? nodes : [schema.nodes.paragraph?.create() ?? schema.topNodeType.create()],
  );
  return { text, sources, separators, origin, doc };
}

/** One top-level node as composed. */
type Chunk = {
  node: PMNode;
  text: string;
  /** Block it stands for (null: new). */
  origin: number | null;
  /** Written with the block's own bytes. */
  kept: boolean;
};

export type Composed = {
  text: string;
  /** Blocks rewritten | added | removed. */
  changed: number;
  /** Line (1-based) where each top-level node starts in `text` (same order as the document). */
  lines: Map<PMNode, number>;
  /** Range of each top-level node in `text`, by its index in the document (`null`: an empty paragraph, not written). */
  ranges: (TextRange | null)[];
};

const isEmptyParagraph = (node: PMNode): boolean =>
  node.type.name === "paragraph" && node.childCount === 0;

const OPENING_FENCE = /^( {0,3}(?:`{3,}|~{3,})[ \t]*)(\S*)/;

/** A code block whose only change is its language: the block's own text with the info word replaced. */
function withFenceLanguage(built: Built, j: number, node: PMNode, core: Core): string | null {
  const source = built.sources[j];
  if (!source || node.type.name !== "code_block" || source.type !== "code_block") return null;
  const match = OPENING_FENCE.exec(source.text);
  if (!match) return null;
  const firstBreak = source.text.indexOf("\n");
  if (firstBreak < 0) return null;
  // same code: the editor's writing without its first line matches the block's own without its first line
  const written = core.write(node);
  const writtenBody = written.slice(written.indexOf("\n") + 1);
  const ownWritten = source.written ?? "";
  if (writtenBody !== ownWritten.slice(ownWritten.indexOf("\n") + 1)) return null;
  const language = String(node.attrs.language ?? "");
  return source.text.replace(OPENING_FENCE, (_, lead: string) => lead + language);
}

type ListMarker = { ordered: boolean; mark: string };
const BULLET_MARKS = ["-", "*", "+"];
const NUMBER_MARKS = [".", ")"];

function listMarker(text: string): ListMarker | null {
  const found = /^ {0,3}(?:([-*+])|\d{1,9}([.)]))(?=[ \t\n]|$)/.exec(text);
  if (!found) return null;
  return found[1] ? { ordered: false, mark: found[1] } : { ordered: true, mark: found[2] ?? "." };
}

/** Top-level item markers of a list text changed to `to` (lines inside items untouched). */
function withMarker(text: string, to: ListMarker): string {
  const lead = /^ {0,3}/.exec(text)?.[0] ?? "";
  return to.ordered
    ? text.replace(new RegExp(`^(${lead})(\\d{1,9})[.)](?=[ \\t]|$)`, "gm"), `$1$2${to.mark}`)
    : text.replace(new RegExp(`^(${lead})[-*+](?=[ \\t]|$)`, "gm"), `$1${to.mark}`);
}

/** Two lists of one kind side by side are one list in Markdown unless their markers differ: keep them apart. */
function keepListsApart(chunks: Chunk[]): void {
  const markers = chunks.map((c) =>
    c.node.type.name === "bullet_list" || c.node.type.name === "ordered_list"
      ? listMarker(c.text)
      : null,
  );
  for (let i = 0; i < chunks.length; i++) {
    const here = markers[i];
    const chunk = chunks[i];
    if (!here || !chunk) continue;
    const before = markers[i - 1];
    const clash = (other: ListMarker | null | undefined) =>
      other && other.ordered === here.ordered && other.mark === here.mark;
    // a kept list gives way only to the list before it
    if (chunk.kept && !clash(before)) continue;
    const taken = new Set<string>();
    for (const k of [i - 1, i + 1]) {
      const other = markers[k];
      if (!other || other.ordered !== here.ordered) continue;
      if (k < i || chunks[k]?.kept) taken.add(other.mark);
    }
    if (!taken.has(here.mark)) continue;
    const mark = (here.ordered ? NUMBER_MARKS : BULLET_MARKS).find((m) => !taken.has(m));
    if (mark === undefined) continue;
    chunk.text = withMarker(chunk.text, { ordered: here.ordered, mark });
    chunk.kept = false;
    markers[i] = { ordered: here.ordered, mark };
  }
}

/**
 * Longest block (own text | writing) spliced; a longer block is written by the editor. Measured near the limit: the
 * alignment stays under 10 ms; comparing meanings reads the block twice per try (~0.1 s at 20,000 characters, up to
 * ~0.45 s at 45,000, once per typing pause). A lower limit would make every edit of a long loose list add a blank
 * line between all its items.
 */
export const MAX_SPLICE = 50_000;
/** Characters the tries of leaving changes out may read (changes * block length): a big table stays quick. */
const LEAVE_OUT_BUDGET = 40_000;

/** `written[a, a + count)` equals `text[b, b + count)`. */
type Run = { a: number; b: number; count: number };
/** `text[from, to)` replaced by `insert`. */
type Hunk = { from: number; to: number; insert: string };

/** Equal runs of a block's writing at load against its own text, both ends included (fixed per block). */
const alignments = new WeakMap<Source, Run[]>();
/** Splice of an edited node (ProseMirror never changes a node in place). */
const splices = new WeakMap<PMNode, { source: Source; text: string | null }>();

function alignment(source: Source, written: string): Run[] {
  let runs = alignments.get(source);
  if (runs === undefined) {
    runs = [{ a: 0, b: 0, count: 0 }];
    for (const op of diff(written.split(""), source.text.split(""))) {
      if (op.kind === "same") runs.push({ a: op.a, b: op.b, count: op.count });
    }
    runs.push({ a: written.length, b: source.text.length, count: 0 });
    alignments.set(source, runs);
  }
  return runs;
}

/**
 * Offset in the block's text of offset `at` in its writing; null inside a part the two write differently. Next to
 * characters only the text has (indentation, a trailing space, an escape), `at` has two places: `late` takes the
 * one after them.
 */
function place(runs: readonly Run[], at: number, late: boolean): number | null {
  let found: number | null = null;
  for (const run of runs) {
    if (run.a > at) break;
    if (at <= run.a + run.count) {
      found = run.b + at - run.a;
      if (!late) break;
    }
  }
  return found;
}

function applied(text: string, hunks: readonly Hunk[]): string {
  let out = "";
  let next = 0;
  for (const hunk of hunks) {
    out += text.slice(next, hunk.from) + hunk.insert;
    next = hunk.to;
  }
  return out + text.slice(next);
}

/**
 * The block's own text with the editor's change to its writing (writing at load -> writing now) applied: characters
 * the person did not touch keep their bytes where the editor writes them differently (escapes, `<url>`, bullets,
 * indentation, blank lines in a list). A change that falls where the two writings differ has no place in the text
 * and is left out. Null (the editor's writing is used) when the result does not read as the editor's writing.
 */
function splice(core: Core, source: Source, node: PMNode): string | null {
  const cached = splices.get(node);
  if (cached?.source === source) return cached.text;
  const text = spliceOf(core, source, node);
  splices.set(node, { source, text });
  return text;
}

function spliceOf(core: Core, source: Source, node: PMNode): string | null {
  const before = source.written;
  const now = core.write(node);
  if (before === null || Math.max(source.text.length, before.length, now.length) > MAX_SPLICE) {
    return null;
  }
  const runs = alignment(source, before);
  const changes = diff(before.split(""), now.split("")).flatMap((op) =>
    op.kind === "change" ? [op] : [],
  );
  let tried: string | null = null;
  // first a deletion keeps the text's own characters at its edges and an insertion goes before them; then the reverse
  for (const inner of [true, false]) {
    const hunks: Hunk[] = [];
    for (const op of changes) {
      const from = place(runs, op.a, op.remove === 0 ? !inner : inner);
      const to = op.remove === 0 ? from : place(runs, op.a + op.remove, !inner);
      if (from === null || to === null || to < from || from < (hunks.at(-1)?.to ?? 0)) continue;
      hunks.push({ from, to, insert: now.slice(op.b, op.b + op.add) });
    }
    // no change placed: the block's own bytes, kept only when the edit does not change what the block reads as
    const out = applied(source.text, hunks);
    if (out === tried) continue;
    tried = out;
    if (sameMeaning(core, out, now)) return leaveOut(core, source.text, hunks, now);
  }
  return null;
}

/**
 * Changes the result reads the same without (the editor re-padding a table's columns) are left out; all of them
 * left out -> the block's own bytes (the edit changed nothing it reads as).
 */
function leaveOut(core: Core, text: string, hunks: readonly Hunk[], now: string): string {
  let kept = hunks;
  if (hunks.length > 1 && hunks.length * text.length <= LEAVE_OUT_BUDGET) {
    for (const hunk of hunks) {
      const without = kept.filter((h) => h !== hunk);
      if (sameMeaning(core, applied(text, without), now)) kept = without;
    }
  }
  return applied(text, kept);
}

/** The text of `doc` (see the header). */
export function compose(core: Core, built: Built, doc: PMNode): Composed {
  const nodes: PMNode[] = [];
  /** Document index of each node in `nodes`. */
  const indexes: number[] = [];
  doc.forEach((node, _, index) => {
    if (isEmptyParagraph(node)) return;
    nodes.push(node);
    indexes.push(index);
  });
  const claimed = new Set<number>();
  const chunks: Chunk[] = nodes.map((node) => {
    const j = built.origin.get(node);
    if (j !== undefined && !claimed.has(j)) {
      claimed.add(j);
      return { node, text: built.sources[j]?.text ?? "", origin: j, kept: true };
    }
    return { node, text: "", origin: null, kept: false };
  });
  // a node equal to an unclaimed block (undo, retyped) is that block
  const byWriting = new Map<string, number[]>();
  const keyOf = (node: PMNode): string =>
    node.type.name === LOCKED ? `${LOCKED}:${String(node.attrs.source)}` : core.write(node);
  built.sources.forEach((s, j) => {
    const key = s.type === LOCKED ? `${LOCKED}:${s.text}` : s.written;
    if (key !== null && !claimed.has(j)) {
      const list = byWriting.get(key) ?? [];
      list.push(j);
      byWriting.set(key, list);
    }
  });
  let lastOrigin = -1;
  for (const chunk of chunks) {
    if (chunk.origin !== null) {
      lastOrigin = chunk.origin;
      continue;
    }
    const candidates = (byWriting.get(keyOf(chunk.node)) ?? []).filter((j) => !claimed.has(j));
    const j = candidates.find((c) => c > lastOrigin) ?? candidates[0];
    if (j !== undefined) {
      claimed.add(j);
      chunk.origin = j;
      chunk.kept = true;
      chunk.text = built.sources[j]?.text ?? "";
      lastOrigin = j;
    }
  }
  // a run of new nodes in the place of as many unclaimed blocks adopts them in order (separators)
  const n = built.sources.length;
  for (let i = 0; i < chunks.length;) {
    if (chunks[i]?.origin !== null) {
      i += 1;
      continue;
    }
    let end = i;
    while (end < chunks.length && chunks[end]?.origin === null) end += 1;
    const before = i === 0 ? -1 : chunks[i - 1]?.origin;
    const after = end === chunks.length ? n : chunks[end]?.origin;
    if (
      typeof before === "number" &&
      typeof after === "number" &&
      after - before - 1 === end - i &&
      chunks.slice(i, end).every((_, k) => !claimed.has(before + 1 + k))
    ) {
      chunks.slice(i, end).forEach((chunk, k) => {
        claimed.add(before + 1 + k);
        chunk.origin = before + 1 + k;
      });
    }
    i = end;
  }
  // texts of changed nodes
  for (const chunk of chunks) {
    if (chunk.kept) continue;
    if (chunk.node.type.name === LOCKED) {
      chunk.text = String(chunk.node.attrs.source);
      chunk.kept = chunk.origin !== null && built.sources[chunk.origin]?.text === chunk.text;
      continue;
    }
    const source = chunk.origin === null ? undefined : built.sources[chunk.origin];
    const own =
      chunk.origin === null || source === undefined
        ? null
        : (withFenceLanguage(built, chunk.origin, chunk.node, core) ??
          splice(core, source, chunk.node));
    chunk.text = own ?? core.write(chunk.node);
  }
  keepListsApart(chunks);

  const separator = (a: Chunk, b: Chunk): string => {
    if (a.origin !== null && b.origin !== null && b.origin === a.origin + 1) {
      const original = built.separators[b.origin] ?? "\n\n";
      if (a.kept && b.kept) return original;
      const sameKind = (c: Chunk) =>
        c.origin !== null && built.sources[c.origin]?.type === c.node.type.name;
      if (sameKind(a) && sameKind(b)) return original;
      return original.includes("\n\n") ? original : "\n\n";
    }
    return "\n\n";
  };

  const lines = new Map<PMNode, number>();
  const ranges: (TextRange | null)[] = Array.from({ length: doc.childCount }, () => null);
  if (chunks.length === 0) {
    return { text: n === 0 ? built.text : (built.separators[n] ?? ""), changed: n, lines, ranges };
  }
  const leading = n === 0 ? "" : (built.separators[0] ?? "");
  const trailing = n === 0 ? built.text : (built.separators[n] ?? "");
  let text = leading;
  let line = 1 + countBreaks(leading);
  chunks.forEach((chunk, i) => {
    if (i > 0) {
      const sep = separator(chunks[i - 1] as Chunk, chunk);
      text += sep;
      line += countBreaks(sep);
    }
    lines.set(chunk.node, line);
    ranges[indexes[i] ?? 0] = { from: text.length, to: text.length + chunk.text.length };
    text += chunk.text;
    line += countBreaks(chunk.text);
  });
  text += trailing;
  const changed = chunks.filter((c) => !c.kept).length + (n - claimed.size);
  return { text, changed, lines, ranges };
}

function countBreaks(text: string): number {
  let count = 0;
  for (let i = 0; i < text.length; i++) if (text.charCodeAt(i) === 10) count += 1;
  return count;
}
