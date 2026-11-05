// @vitest-environment jsdom
import "./jsdomShims";
import { undo } from "@milkdown/kit/prose/history";
import { AllSelection, TextSelection } from "@milkdown/kit/prose/state";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Segment } from "../../../api/generated/Segment";
import type { TextEdit } from "../session.svelte";
import { buildDoc, compose, type Built, type Composed } from "./compose";
import { createCore, type Core } from "./core";
import { applyEdits, blockOf, scopeIn } from "./edits";

let core: Core;
let built: Built;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host });
});

afterAll(async () => {
  await core.destroy();
});

/** Blank-line separated runs (test texts only), as in compose.test.ts. */
function segmentsOf(text: string): Segment[] {
  const lines = text.split("\n");
  const out: Segment[] = [];
  let start = -1;
  lines.forEach((line, i) => {
    const filled = line.trim() !== "";
    if (filled && start < 0) start = i;
    if (!filled && start >= 0) {
      out.push({ lineStart: start + 1, lineEnd: i, kind: "paragraph", lock: null });
      start = -1;
    }
  });
  if (start >= 0)
    out.push({ lineStart: start + 1, lineEnd: lines.length, kind: "paragraph", lock: null });
  return out;
}

async function load(text: string): Promise<void> {
  built = await buildDoc(core, text, segmentsOf(text));
  core.setDoc(built.doc);
}

const now = (): Composed => compose(core, built, core.view.state.doc);

/** A word fix: `word` (first after `after`) becomes `insert`. */
function fix(text: string, word: string, insert: string, after = 0): TextEdit {
  const from = text.indexOf(word, after);
  if (from < 0) throw new Error(`no ${word}`);
  return { from, to: from + word.length, insert, expect: word };
}

/** Document position of `word` in the editor. */
function posOf(word: string): number {
  let found = -1;
  core.view.state.doc.descendants((node, pos) => {
    if (found >= 0 || !node.isText) return;
    const at = (node.text ?? "").indexOf(word);
    if (at >= 0) found = pos + at;
  });
  if (found < 0) throw new Error(`no ${word}`);
  return found;
}

function select(from: number, to = from): void {
  const view = core.view;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, from, to)));
}

// odd but kept styles: `_` emphasis, `__` strong, `*` bullets, trailing spaces, extra blank lines
const DOC =
  "# Teh _title_\n\nSecond paragrap with *emph* here.\n\n\n* item _one_  \n* item two\n\nLast __bold__ line.\n";
const BLOCK_2 = "Second paragrap with *emph* here.";

describe("Edit-mode ranges", () => {
  it("each top-level block's range reads its own text in the composed text", async () => {
    await load(DOC);
    const composed = now();
    expect(composed.text).toBe(DOC);
    const texts = composed.ranges.map((r) => (r ? composed.text.slice(r.from, r.to) : null));
    expect(texts).toEqual([
      "# Teh _title_",
      BLOCK_2,
      "* item _one_  \n* item two",
      "Last __bold__ line.",
    ]);
    expect(blockOf(composed, DOC.indexOf("paragrap"), DOC.indexOf("here"))).toBe(1);
    // the blank lines between two blocks belong to none
    expect(blockOf(composed, DOC.indexOf("\n\n\n"), DOC.indexOf("\n\n\n") + 1)).toBeNull();
  });
});

describe("what a correction works on in the Edit-mode editor", () => {
  it("paragraph: the whole top-level block at the caret", async () => {
    await load(DOC);
    select(posOf("paragrap") + 2);
    const start = DOC.indexOf(BLOCK_2);
    expect(scopeIn(core.view.state, now(), "paragraph")).toEqual({
      from: start,
      to: start + BLOCK_2.length,
    });
    select(posOf("two"));
    const list = DOC.indexOf("* item _one_");
    expect(scopeIn(core.view.state, now(), "paragraph")).toEqual({
      from: list,
      to: DOC.indexOf("two") + 3,
    });
  });

  it("selection: the whole blocks it touches; none selected = nothing", async () => {
    await load(DOC);
    select(posOf("paragrap"));
    expect(scopeIn(core.view.state, now(), "selection")).toBeNull();
    select(posOf("with"), posOf("one"));
    expect(scopeIn(core.view.state, now(), "selection")).toEqual({
      from: DOC.indexOf(BLOCK_2),
      to: DOC.indexOf("two") + 3,
    });
    const view = core.view;
    view.dispatch(view.state.tr.setSelection(new AllSelection(view.state.doc)));
    expect(scopeIn(view.state, now(), "selection")).toEqual({
      from: 0,
      to: DOC.length - 1,
    });
  });

  it("document: the whole text", async () => {
    await load(DOC);
    expect(scopeIn(core.view.state, now(), "document")).toEqual({ from: 0, to: DOC.length });
  });
});

describe("checked edits in the Edit-mode editor", () => {
  it("a word fix rewrites only its block: every other block keeps its bytes", async () => {
    await load(DOC);
    const before = core.view.state.doc;
    expect(applyEdits(core, now(), [fix(DOC, "paragrap", "paragraph")])).toBe(true);
    const after = now();
    expect(after.text).toBe(DOC.replace("paragrap ", "paragraph "));
    expect(after.changed).toBe(1);
    // the other nodes are the same objects
    [0, 2, 3].forEach((i) => expect(core.view.state.doc.child(i)).toBe(before.child(i)));
  });

  it("the touched block is written by the editor, as typing would (its `_` emphasis stays)", async () => {
    const text = "Kept _one_.\n\nFixd _two_.\n\nKept _three_.\n";
    await load(text);
    expect(applyEdits(core, now(), [fix(text, "Fixd", "Fixed")])).toBe(true);
    expect(now().text).toBe("Kept _one_.\n\nFixed _two_.\n\nKept _three_.\n");
  });

  it("edits in two blocks go in together and come out with one undo", async () => {
    await load(DOC);
    const edits = [fix(DOC, "Teh", "The"), fix(DOC, "paragrap", "paragraph")];
    expect(applyEdits(core, now(), edits)).toBe(true);
    expect(now().text).toBe(DOC.replace("Teh", "The").replace("paragrap ", "paragraph "));
    const view = core.view;
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(now().text).toBe(DOC);
    expect(now().changed).toBe(0);
  });

  it("typing right after a fix is an undo step of its own", async () => {
    await load(DOC);
    expect(applyEdits(core, now(), [fix(DOC, "paragrap", "paragraph")])).toBe(true);
    const view = core.view;
    const at = posOf("here");
    view.dispatch(view.state.tr.insertText("x", at));
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(now().text).toBe(DOC.replace("paragrap ", "paragraph "));
  });

  it("nothing changes when a range no longer reads what was expected", async () => {
    await load(DOC);
    const before = core.view.state.doc;
    const stale = { ...fix(DOC, "paragrap", "paragraph"), expect: "paragrab" };
    expect(applyEdits(core, now(), [fix(DOC, "Teh", "The"), stale])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });

  it("refuses an edit across two blocks, between blocks, or in a locked block", async () => {
    const text = "One here.\n\nTwo there.\n\nMath $x_1$ here.\n";
    await load(text);
    const before = core.view.state.doc;
    const across = text.indexOf("here.");
    const end = text.indexOf("Two") + 3;
    const span = text.slice(across, end);
    expect(applyEdits(core, now(), [{ from: across, to: end, insert: "x", expect: span }])).toBe(
      false,
    );
    const gap = text.indexOf("\n\n");
    expect(applyEdits(core, now(), [{ from: gap, to: gap + 1, insert: " ", expect: "\n" }])).toBe(
      false,
    );
    expect(applyEdits(core, now(), [fix(text, "Math", "Maths")])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });

  it("refuses a fix that would make its block read as something else", async () => {
    const text = "A list:\n\nSee r here.\n\n[r]: http://x\n";
    await load(text);
    const before = core.view.state.doc;
    // `[r]` would use the definition below: the editor would write it escaped
    expect(applyEdits(core, now(), [fix(text, "r here", "[r] here")])).toBe(false);
    // a paragraph turned into a heading
    expect(applyEdits(core, now(), [fix(text, "A list:", "# A list:")])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });
});
