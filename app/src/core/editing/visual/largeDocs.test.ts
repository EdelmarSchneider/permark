// @vitest-environment jsdom
// Edit mode over large documents. Marks of a whole document are placed in one pass (`docRanges`): each place equals
// the one a range looked up alone found, by the way that lookup worked before (kept below as the reference), within
// a time a pass on every typing pause allows. An edit reads the whole text again only when a touched block could
// use a picture by reference (`mayUsePicture`: never false for a block that whole reading finds one in), and takes |
// refuses as the edit that always read it did (reference below too).
import "./jsdomShims";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { closeHistory } from "@milkdown/kit/prose/history";
import { TextSelection, type EditorState, type Transaction } from "@milkdown/kit/prose/state";
import { $prose } from "@milkdown/kit/utils";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import {
  misspelledRanges,
  paragraphSegments,
  paragraphsText,
  tableText,
} from "../../testing/editDocs";
import type { TextEdit, TextRange } from "../session.svelte";
import { checkEdits } from "../textEdits";
import { blocksUsingReferences, classify, mayUsePicture } from "./classify";
import { buildDoc, compose, type Built, type Composed } from "./compose";
import { createCore, LOCKED, type Core } from "./core";
import { blockOf, changeable, docRange, docRanges, editsTr } from "./edits";
import { pieceSources } from "./inlineSource";
import { markAt, marksPlugin, rangeAt, setMarks } from "./marks";

let core: Core;
let built: Built;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host, plugins: [$prose(() => marksPlugin())] });
});

afterAll(async () => {
  await core.destroy();
});

async function load(text: string): Promise<Composed> {
  built = await buildDoc(core, text, paragraphSegments(text));
  core.setDoc(built.doc);
  return now();
}

const now = (): Composed => compose(core, built, core.view.state.doc);

/** Small deterministic random (tests repeat). */
function rng(seed: number): (n: number) => number {
  let s = seed >>> 0;
  return (n) => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return Math.floor((s / 2 ** 32) * n);
  };
}

// ---- the way a range was looked up before: one block scan, one shown text and a copy per candidate per range ----

const CONTEXT = 24;
const MARKUP = /[\\*_~`[\]()#>|]/g;

function shownBefore(node: PMNode, start: number): { text: string; at: number[] } {
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

function tailBefore(markdown: string, shown: string): number {
  const a = markdown.replace(MARKUP, "");
  const b = shown.replace(MARKUP, "");
  let n = 0;
  while (n < a.length && n < b.length && a[a.length - 1 - n] === b[b.length - 1 - n]) n += 1;
  return n;
}

function headBefore(markdown: string, shown: string): number {
  const a = markdown.replace(MARKUP, "");
  const b = shown.replace(MARKUP, "");
  let n = 0;
  while (n < a.length && n < b.length && a[n] === b[n]) n += 1;
  return n;
}

function docRangeBefore(doc: PMNode, composed: Composed, range: TextRange): TextRange | null {
  const index = blockOf(composed, range.from, range.to);
  const block = index === null ? null : composed.ranges[index];
  const node = index === null ? null : doc.maybeChild(index);
  if (index === null || !block || !node || node.type.name === LOCKED) return null;
  let start = 0;
  for (let i = 0; i < index; i++) start += doc.child(i).nodeSize;
  const shown = shownBefore(node, start);
  const markdown = composed.text.slice(block.from, block.to);
  const rel = range.from - block.from;
  const pieces = composed.pieces[index] ?? [];
  if (pieces.some((span) => range.from < span.to && range.to > span.from)) return null;
  const needle = composed.text.slice(range.from, range.to);
  const before = markdown.slice(Math.max(0, rel - CONTEXT), rel);
  const after = markdown.slice(rel + needle.length, rel + needle.length + CONTEXT);
  const expected = (rel * shown.text.length) / Math.max(1, markdown.length);
  let best: { at: number; score: number; off: number } | null = null;
  for (let at = shown.text.indexOf(needle); at >= 0; at = shown.text.indexOf(needle, at + 1)) {
    const score =
      tailBefore(before, shown.text.slice(Math.max(0, at - CONTEXT), at)) +
      headBefore(after, shown.text.slice(at + needle.length, at + needle.length + CONTEXT));
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

// ---- the edit as it was before: the whole edited text read again whenever it holds a definition ----

function startsAfterBefore(composed: Composed, edits: readonly TextEdit[]): number[] {
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

function editsTrBefore(
  state: EditorState,
  composed: Composed,
  edits: readonly TextEdit[],
): Transaction | null {
  const text = composed.text;
  const sorted = checkEdits(edits, text.length, (from, to) => text.slice(from, to));
  if (sorted === null) return null;
  const byBlock = new Map<number, { node: PMNode; range: TextRange; edits: TextEdit[] }>();
  for (const edit of sorted) {
    const index = blockOf(composed, edit.from, edit.to);
    const range = index === null ? null : composed.ranges[index];
    const node = index === null ? null : state.doc.maybeChild(index);
    if (index === null || !range || !node || node.type.name === LOCKED) return null;
    const held = byBlock.get(index);
    if (held) held.edits.push(edit);
    else byBlock.set(index, { node, range, edits: [edit] });
  }
  let edited = text;
  for (const edit of [...sorted].reverse()) {
    edited = edited.slice(0, edit.from) + edit.insert + edited.slice(edit.to);
  }
  const written = composed.ranges.flatMap((r, index) => (r === null ? [] : [index]));
  const usesReference = blocksUsingReferences(
    core,
    edited,
    startsAfterBefore(composed, sorted),
  ).blocks;
  const at: number[] = [];
  state.doc.forEach((_, offset) => at.push(offset));
  const replaced: { from: number; to: number; node: PMNode }[] = [];
  for (const [index, { node, range, edits: blockEdits }] of byBlock) {
    const pos = at[index];
    if (pos === undefined) return null;
    let markdown = text.slice(range.from, range.to);
    for (const edit of [...blockEdits].reverse()) {
      const from = edit.from - range.from;
      markdown = markdown.slice(0, from) + edit.insert + markdown.slice(edit.to - range.from);
    }
    const verdict = classify(core, markdown, null, usesReference.has(written.indexOf(index)));
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
  for (const { from, to, node } of replaced.sort((a, b) => b.from - a.from)) {
    tr.replaceWith(from, to, node);
  }
  return closeHistory(tr);
}

/** What a transaction makes of the document (`null`: refused). */
const outcome = (tr: Transaction | null): unknown => (tr === null ? null : tr.doc.toJSON());

// -----------------------------------------------------------------------------------------------------------------

/** Ranges of every kind in `text`: misspelled words, random stretches (between | across blocks, over markup and
 *  pieces), insertion points, whole blocks. */
function mixedRanges(composed: Composed, words: number, random: number, seed: number): TextRange[] {
  const text = composed.text;
  const pick = rng(seed);
  const out = misspelledRanges(text).slice(0, words);
  for (let i = 0; i < random; i++) {
    const from = pick(text.length + 1);
    const kind = pick(4);
    if (kind === 0) out.push({ from, to: from });
    else if (kind === 1) {
      const block = composed.ranges[pick(composed.ranges.length)];
      if (block) out.push(pick(2) === 0 ? block : { from: block.to, to: block.to });
    } else out.push({ from, to: Math.min(text.length, from + 1 + pick(40)) });
  }
  return out.sort((a, b) => a.from - b.from || a.to - b.to);
}

describe("Edit marks of a whole document in one pass", () => {
  it("each range placed where a lookup of it alone placed it before (1000 paragraphs, 4000 ranges)", async () => {
    const text = `${paragraphsText(1000)}\nA note[^a] and teh end, $x^2$ too.\n\n[^a]: The note.\n\n[ref]: https://example.com\n`;
    await load(text);
    // empty paragraphs between blocks: not written, no range of their own
    const view = core.view;
    const paragraph = view.state.schema.nodes.paragraph;
    if (!paragraph) throw new Error("no paragraph");
    for (const index of [800, 500, 10]) {
      let pos = 0;
      for (let i = 0; i < index; i++) pos += view.state.doc.child(i).nodeSize;
      view.dispatch(view.state.tr.insert(pos, paragraph.create()));
    }
    const composed = now();
    expect(composed.ranges.filter((r) => r === null)).toHaveLength(3);
    const ranges = mixedRanges(composed, 2_800, 1_200, 7);
    expect(ranges.length).toBeGreaterThanOrEqual(3_800);
    const doc = view.state.doc;
    const started = performance.now();
    const placed = docRanges(doc, composed, ranges);
    const took = performance.now() - started;
    expect(placed).toEqual(ranges.map((range) => docRangeBefore(doc, composed, range)));
    expect(placed.filter((p) => p !== null).length).toBeGreaterThan(2_800);
    // one range alone: the same lookup
    for (const range of ranges.slice(0, 500)) {
      expect(docRange(doc, composed, range)).toEqual(docRangeBefore(doc, composed, range));
    }
    // measured ~5-10 ms; the bound leaves room for a slow machine
    expect(took).toBeLessThan(100);
  });

  it("in one 30 KB table, every range is placed as before, and fast", async () => {
    const text = tableText(600);
    const composed = await load(text);
    expect(composed.ranges).toHaveLength(1);
    const doc = core.view.state.doc;
    const ranges = misspelledRanges(text, 4);
    const started = performance.now();
    const placed = docRanges(doc, composed, ranges);
    const took = performance.now() - started;
    // the reference copies text per candidate per range, slow here: a sample of the words, every kind of the rest
    const pick = rng(3);
    const sample = [
      ...Array.from({ length: 150 }, () => ranges[pick(ranges.length)]),
      { from: 0, to: 0 },
      { from: text.length - 2, to: text.length - 2 },
      { from: 40, to: 120 },
      { from: 3, to: 7 },
    ].filter((r) => r !== undefined);
    expect(docRanges(doc, composed, sample)).toEqual(
      sample.map((range) => docRangeBefore(doc, composed, range)),
    );
    expect(placed.every((p) => p !== null)).toBe(true);
    // measured ~25 ms (every candidate of a word repeated in each row is compared)
    expect(took).toBeLessThan(300);
  });

  it("the range under the caret before marking: what the mark under it answers once marked", async () => {
    const text = `${paragraphsText(60)}\nA note[^a] and teh end.\n\n[^a]: The note.\n`;
    const composed = await load(text);
    // marks of one list never overlap (a spelling list, a review's suggestions)
    const ranges: TextRange[] = [];
    for (const range of mixedRanges(composed, 300, 60, 11)) {
      if (range.from >= (ranges.at(-1)?.to ?? 0)) ranges.push(range);
    }
    const view = core.view;
    const pick = rng(5);
    const carets: number[] = [];
    for (let i = 0; i < 200; i++) {
      const pos = 1 + pick(view.state.doc.content.size - 1);
      if (view.state.doc.resolve(pos).parent.inlineContent) carets.push(pos);
    }
    const caretAt = (pos: number): void => {
      view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos)));
    };
    // marks map through a caret move unchanged: one marking pass serves every caret
    setMarks(view, composed, [], null, () => {});
    const before = carets.map((pos) => {
      caretAt(pos);
      return rangeAt(view.state, composed, ranges);
    });
    setMarks(view, composed, ranges, null, () => {});
    const marked = carets.map((pos) => {
      caretAt(pos);
      return markAt(view.state);
    });
    expect(marked).toEqual(before);
    expect(marked.filter((index) => index !== null).length).toBeGreaterThan(20);
  });
});

/** Single word edits all over `text`: `word` (each of its places, every `step`-th) becomes `insert`. */
function wordEdits(text: string, word: string, insert: string, step: number): TextEdit[] {
  const out: TextEdit[] = [];
  let seen = 0;
  for (let at = text.indexOf(word); at >= 0; at = text.indexOf(word, at + 1)) {
    if (seen++ % step === 0) out.push({ from: at, to: at + word.length, insert, expect: word });
  }
  return out;
}

describe("an accepted fix in a document with definitions", () => {
  it("is not a read of the whole text when no touched block could use a picture by reference", async () => {
    const text = `${paragraphsText(500)}\nA note[^a] here.\n\n[^a]: The note.\n`;
    const composed = await load(text);
    const edits = wordEdits(composed.text, "teh", "the", 11).slice(0, 50);
    expect(edits).toHaveLength(50);
    const parsed = vi.spyOn(core, "parseRaw");
    const started = performance.now();
    try {
      for (const edit of edits) {
        expect(changeable(core, composed, edit, edit.insert)).toBe(true);
      }
      expect(parsed).not.toHaveBeenCalled();
    } finally {
      parsed.mockRestore();
    }
    // measured ~15 ms for all 50; each fix was a read of the whole text (~25 ms here) before
    expect(performance.now() - started).toBeLessThan(200);
  });

  it("a block whose stretch holds no `![` is never one a whole reading finds using a picture by reference", () => {
    const parts = ["![a][img]", "![b]", "![c][]", "!", "[", "]", "`", "\\", "x", " ", "\n", "\n\n"];
    const defs = "\n\n[img]: a.png\n\n[b]: b.png\n\n[c]: c.png\n";
    const pick = rng(23);
    let found = 0;
    for (let i = 0; i < 400; i++) {
      let text = "";
      for (let k = 0; k < 40; k++) text += parts[pick(parts.length)] ?? "";
      text += defs;
      const starts = [0];
      for (let at = pick(6) + 1; at < text.length; at += 1 + pick(12)) starts.push(at);
      const blocks = blocksUsingReferences(core, text, starts).blocks;
      found += blocks.size;
      starts.forEach((_, block) => {
        if (!mayUsePicture(text, starts, block)) expect(blocks.has(block)).toBe(false);
      });
    }
    expect(found).toBeGreaterThan(100);
  });

  it("takes | refuses every edit as the edit that always read the whole text did", async () => {
    const blocks = [
      "Plain words teh here and more words.",
      "A picture by reference ![alt][img] in text.",
      "A label ![alt][rex] not defined, and teh rest.",
      "An inline ![pic](a.png) picture and teh word.",
      "A footnote ref[^n] here and teh word.",
      "A link [text][img] by reference and teh word.",
      "Text with `code` and teh word\\! escaped.",
      "Ends with a bang!",
      "[img]: https://example.com/a.png",
      "[^n]: The note.",
      "Another plain teh paragraph.",
    ];
    const text = `${[...blocks, ...blocks].join("\n\n")}\n`;
    const composed = await load(text);
    const state = core.view.state;
    // the label made a defined one: a picture by reference now, kept as written; a fix beside it still goes in
    const label = text.indexOf("rex");
    expect(changeable(core, composed, { from: label, to: label + 3 }, "img")).toBe(false);
    const beside = text.indexOf("teh", label);
    expect(changeable(core, composed, { from: beside, to: beside + 3 }, "the")).toBe(true);
    const inserts = ["the", "![", "[", "]", "!", "`", "\\", "]:", "img", "![x][img]", "x]: y", ""];
    const pick = rng(17);
    let taken = 0;
    let refused = 0;
    for (let i = 0; i < 200; i++) {
      const count = 1 + (pick(4) === 0 ? pick(3) : 0);
      const edits: TextEdit[] = [];
      for (let k = 0; k < count; k++) {
        const from = pick(text.length);
        const to = Math.min(text.length, from + pick(6));
        edits.push({
          from,
          to,
          insert: inserts[pick(inserts.length)] ?? "",
          expect: text.slice(from, to),
        });
      }
      // label edits: `rex` -> `img` makes a defined picture by reference out of text
      if (i % 25 === 0) {
        const at = text.indexOf("rex", pick(text.length / 2));
        if (at >= 0)
          edits.splice(0, edits.length, { from: at, to: at + 3, insert: "img", expect: "rex" });
      }
      const was = outcome(editsTrBefore(state, composed, edits));
      expect(outcome(editsTr(core, state, composed, edits))).toEqual(was);
      if (was === null) refused += 1;
      else taken += 1;
    }
    // both answers met often enough to mean something
    expect(taken).toBeGreaterThan(50);
    expect(refused).toBeGreaterThan(50);
  });
});
