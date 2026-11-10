// @vitest-environment jsdom
// The piece review on the Edit-mode editor itself: what stays of it when the text around a suggestion changes. The
// editor writes a block again when it is edited: a table whose cells changed width gets its columns padded anew
// (spaces and the alignment row's dashes move, never a cell's text). Suggestions in its other cells, and pieces not
// answered yet, stay and still go into their own cell; so do suggestions around words typed elsewhere in their
// paragraph. A change of a suggestion's own words, or of the word next to them, drops it.
import "../../../core/editing/visual/jsdomShims";
import { $prose } from "@milkdown/kit/utils";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createMockApi } from "../../../api/mock";
import type { Segment } from "../../../api/generated/Segment";
import { buildDoc, compose, type Built, type Composed } from "../../../core/editing/visual/compose";
import { createCore, type Core } from "../../../core/editing/visual/core";
import { applyEdits, scopeIn, selectRange } from "../../../core/editing/visual/edits";
import { markAt, marksPlugin, setMarks } from "../../../core/editing/visual/marks";
import { PieceReview, type ReviewConfig, type Session } from "./review.svelte";
import { pieces } from "./segment";
import { around } from "./suggestion";

let core: Core;
let built: Built;
let store: PieceReview;
/** What the run was sent. */
let run: { run: string; pieces: { id: number; text: string }[] };

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host, plugins: [$prose(() => marksPlugin())] });
});

afterAll(async () => {
  await core.destroy();
});

const now = (): Composed => compose(core, built, core.view.state.doc);

/** The editing session as the Edit-mode editor serves it (`VisualEditor.svelte`). */
function editorSession(): Session {
  return {
    path: "docs/a.md",
    mode: "edit",
    state: { s: "loading" },
    text: "",
    current: () => now().text,
    scope: (kind) => scopeIn(core.view.state, now(), kind),
    apply: (edits) => applyEdits(core, now(), edits),
    reveal: (range) => void selectRange(core.view, now(), range),
    marks: (ranges, current, pick) => setMarks(core.view, now(), ranges, current, pick),
    markAt: () => markAt(core.view.state),
    composing: () => false,
    focus: () => {},
  };
}

/** A plugin whose answer names parts of the piece and what goes there: `part>fix`, one per `;`. */
const NAMED: ReviewConfig = {
  plugin: "correction",
  prepare: pieces,
  judge: (doc, piece, output) => ({
    kind: "ok",
    suggestions: output.split(";").map((fix) => {
      const [part = "", insert = ""] = fix.split(">");
      const from = piece.from + piece.original.indexOf(part);
      const to = from + part.length;
      return { from, to, original: part, insert, ...around(doc, from, to) };
    }),
  }),
  runArgs: () => ({}),
};

/** One paragraph segment per line run `first..last` (1-based). */
function blocks(...runs: [first: number, last: number][]): Segment[] {
  return runs.map(([lineStart, lineEnd]) => ({
    lineStart,
    lineEnd,
    kind: "paragraph",
    lock: null,
  }));
}

/** Opens `text` in the editor and starts a run over all of it. */
async function reviewOf(text: string, segments: Segment[]): Promise<void> {
  built = await buildDoc(core, text, segments);
  core.setDoc(built.doc);
  const mock = createMockApi();
  mock.probe.correction.stepMs = 1e9;
  store = new PieceReview(NAMED, mock, editorSession());
  await store.ask();
  await store.runOver("document");
  const sent = mock.probe.correction.runs.at(-1);
  if (!sent) throw new Error("no run sent");
  run = sent;
}

/** The model's answer for the `nth` piece that reads `text`. */
function answer(text: string, output: string, nth = 0): void {
  const piece = run.pieces.filter((p) => p.text === text)[nth];
  if (!piece) throw new Error(`no piece ${text}`);
  store.news({
    run: run.run,
    phase: { kind: "piece", id: piece.id, result: { kind: "output", text: output } },
  });
}

function finish(): void {
  store.news({ run: run.run, phase: { kind: "done" } });
}

const pending = (): string[] =>
  (store.review?.suggestions ?? []).map((s) => `${s.original}>${s.insert}`);

/** What each pending suggestion's range reads now, and its line. */
function where(): string[] {
  const text = now().text;
  return (store.review?.suggestions ?? []).map(
    (s) => `${text.slice(s.from, s.to)} @ line ${text.slice(0, s.from).split("\n").length}`,
  );
}

/** Types `insert` in the editor, `offset` chars into the first `word`. */
function type(word: string, insert: string, offset = 0): void {
  const view = core.view;
  let at = -1;
  view.state.doc.descendants((node, pos) => {
    if (at >= 0 || !node.isText) return;
    const found = (node.text ?? "").indexOf(word);
    if (found >= 0) at = pos + found;
  });
  if (at < 0) throw new Error(`no ${word}`);
  view.dispatch(view.state.tr.insertText(insert, at + offset));
}

/** The text with column padding and the alignment row's dashes aside. */
const shape = (text: string): string => text.replace(/ +/g, " ").replace(/-+/g, "-");

describe("suggestions in the cells of one table, in the Edit-mode editor", () => {
  // columns padded by hand (not how the editor writes them); the same text in two cells
  const TABLE = [
    "| Name    | Note      |",
    "| :------ | --------: |",
    "| teh cat | wrng      |",
    "| other   | recieve   |",
    "| thrd    | wrng      |",
  ].join("\n");
  const TEXT = `Kept _one_.\n\n${TABLE}\n\nKept _two_.\n`;

  it("accepting two that change their cell's width leaves the others pending, each still about its own cell", async () => {
    await reviewOf(TEXT, blocks([1, 1], [3, 7], [9, 9]));
    answer("teh cat", "teh cat>the big cat");
    answer("wrng", "wrng>wrong", 0);
    answer("recieve", "recieve>got");
    answer("thrd", "thrd>third one");
    answer("wrng", "wrng>wrong", 1);
    expect(where()).toEqual([
      "teh cat @ line 5",
      "wrng @ line 5",
      "recieve @ line 6",
      "thrd @ line 7",
      "wrng @ line 7",
    ]);

    store.accept();
    store.go(1);
    store.accept();
    // the second width change: the editor wrote the table its own way, not only the cell
    const accepted = TEXT.replace("teh cat", "the big cat").replace("recieve", "got");
    expect(now().text).not.toBe(accepted);
    expect(shape(now().text)).toBe(shape(accepted));
    expect(pending()).toEqual(["wrng>wrong", "thrd>third one", "wrng>wrong"]);
    expect(where()).toEqual(["wrng @ line 5", "thrd @ line 7", "wrng @ line 7"]);
    expect(store.review?.outdated).toBe(0);

    finish();
    store.acceptAll();
    const after = now().text;
    const replaced = accepted.replaceAll("wrng", "wrong").replace("thrd", "third one");
    expect(shape(after)).toBe(shape(replaced));
    // every other block keeps its bytes
    const from = TEXT.indexOf(TABLE);
    expect(after.slice(0, from)).toBe(TEXT.slice(0, from));
    expect(after.endsWith(TEXT.slice(from + TABLE.length))).toBe(true);
    expect(store.state.kind).toBe("closed");
  });

  it("a cell answered only after the table was padded anew still gets its suggestion, in its own cell", async () => {
    await reviewOf(TEXT, blocks([1, 1], [3, 7], [9, 9]));
    answer("teh cat", "teh cat>the big cat");
    answer("recieve", "recieve>got");
    store.accept();
    store.accept();
    const accepted = TEXT.replace("teh cat", "the big cat").replace("recieve", "got");
    expect(now().text).not.toBe(accepted);
    answer("other", "other>another");
    answer("wrng", "wrng>wrong", 1);
    expect(where()).toEqual(["other @ line 6", "wrng @ line 7"]);
    finish();
    store.acceptAll();
    expect(shape(now().text)).toBe(
      shape(accepted.replace("other", "another").replace(/wrng(?!.*wrng)/s, "wrong")),
    );
  });
});

describe("typing while suggestions are pending, in the Edit-mode editor", () => {
  const TEXT = "Intro line.\n\nShe recieve mail from teh office today.\n";

  async function reviewed(): Promise<void> {
    await reviewOf(TEXT, blocks([1, 1], [3, 3]));
    answer("She recieve mail from teh office today.", "recieve>receive;teh>the");
    finish();
    expect(pending()).toEqual(["recieve>receive", "teh>the"]);
  }

  it("words typed elsewhere in the paragraph keep its suggestions, each on its own words", async () => {
    await reviewed();
    type("mail", "e-");
    store.sync();
    expect(where()).toEqual(["recieve @ line 3", "teh @ line 3"]);
    // typed before and after them before the review looks again: one change that spans both
    type("She", "Now ");
    type("today", "just ");
    store.sync();
    expect(now().text).toContain("Now She recieve e-mail from teh office just today.");
    expect(where()).toEqual(["recieve @ line 3", "teh @ line 3"]);
    expect(store.review?.outdated).toBe(0);
    store.acceptAll();
    expect(now().text).toBe("Intro line.\n\nNow She receive e-mail from the office just today.\n");
  });

  it("a letter typed right at a suggestion's edge drops it, also inside a wider change", async () => {
    await reviewed();
    type("She", "So ");
    type("teh", "x", 3);
    store.sync();
    expect(now().text).toContain("So She recieve mail from tehx office");
    expect(pending()).toEqual(["recieve>receive"]);
    expect(store.review?.outdated).toBe(1);
  });

  it("typing inside a suggestion's words drops it and moves the one after", async () => {
    await reviewed();
    type("recieve", "x", 3);
    store.sync();
    expect(pending()).toEqual(["teh>the"]);
    expect(where()).toEqual(["teh @ line 3"]);
    expect(store.review?.outdated).toBe(1);
  });
});
