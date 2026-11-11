// @vitest-environment jsdom
// The piece review on the Edit-mode editor itself: what stays of it when the text around a suggestion changes. The
// editor writes a block again when it is edited: a table whose cells changed width gets its columns padded anew
// (spaces and the alignment row's dashes move, never a cell's text). Suggestions in its other cells, and pieces not
// answered yet, stay and still go into their own cell; so do suggestions around words typed elsewhere in their
// paragraph. A change of a suggestion's own words, or of the word next to them, drops it. An HTML element written in
// a line is kept as written by the editor: its text is not sent, so an answer goes in around it.
import "../../../core/editing/visual/jsdomShims";
import { undo } from "@milkdown/kit/prose/history";
import { NodeSelection } from "@milkdown/kit/prose/state";
import { $prose } from "@milkdown/kit/utils";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createMockApi } from "../../../api/mock";
import type { Segment } from "../../../api/generated/Segment";
import { buildDoc, compose, type Built, type Composed } from "../../../core/editing/visual/compose";
import { createCore, type Core } from "../../../core/editing/visual/core";
import { applyEdits, changeable, scopeIn, selectRange } from "../../../core/editing/visual/edits";
import { markAt, marksPlugin, setMarks } from "../../../core/editing/visual/marks";
import { PieceReview, type ReviewConfig, type Session } from "./review.svelte";
import { pieces, restore } from "./segment";
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
    changeable: (range, insert) => changeable(core, now(), range, insert),
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

/** A plugin whose answer stands for the whole piece (a translation): one suggestion, the markers put back wherever
 *  the answer has them. */
const WHOLE: ReviewConfig = {
  plugin: "correction",
  prepare: pieces,
  judge: (doc, piece, output) => {
    const insert = restore(output, piece, "anyOrder");
    if (insert === null) return { kind: "discard", reason: "markers" };
    const { from, to, original } = piece;
    return { kind: "ok", suggestions: [{ from, to, original, insert, ...around(doc, from, to) }] };
  },
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

/** What the backend was asked to run. */
let runs: (typeof run)[] = [];

/** Opens `text` in the editor, with a review of it (set up as `config`) that has not run yet. */
async function opened(text: string, segments: Segment[], config = NAMED): Promise<void> {
  built = await buildDoc(core, text, segments);
  core.setDoc(built.doc);
  const mock = createMockApi();
  mock.probe.correction.stepMs = 1e9;
  runs = mock.probe.correction.runs;
  store = new PieceReview(config, mock, editorSession());
  await store.ask();
}

/** Opens `text` in the editor and starts a run over all of it. */
async function reviewOf(text: string, segments: Segment[], config = NAMED): Promise<void> {
  await opened(text, segments, config);
  await store.runOver("document");
  const sent = runs.at(-1);
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

describe("blocks the Edit-mode editor keeps as written", () => {
  // an alert and a footnote's text: prose the reader shows, in blocks Edit mode does not write
  const TEXT = [
    "Frist line teh.",
    "",
    "> [!NOTE]",
    "> Alert teh text.",
    "",
    "Scond line[^1].",
    "",
    "[^1]: Footnote teh text.",
    "",
  ].join("\n");
  const SEGMENTS: Segment[] = [
    { lineStart: 1, lineEnd: 1, kind: "paragraph", lock: null },
    { lineStart: 3, lineEnd: 4, kind: "alert", lock: "kind" },
    { lineStart: 6, lineEnd: 6, kind: "paragraph", lock: null },
    { lineStart: 8, lineEnd: 8, kind: "footnote", lock: "footnote" },
  ];

  /** Selects the top-level block `index` as a click on a kept block does. */
  function selectBlock(index: number): void {
    const view = core.view;
    let at = 0;
    for (let i = 0; i < index; i++) at += view.state.doc.child(i).nodeSize;
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, at)));
  }

  it("their text is not sent: only what an accept could put in; the bar is told how many were left out", async () => {
    await reviewOf(TEXT, SEGMENTS);
    expect(run.pieces.map((piece) => piece.text)).toEqual(["Frist line teh.", "Scond line⟦1⟧."]);
    expect(store.review).toMatchObject({ locked: 2, outdated: 0 });
    answer("Frist line teh.", "Frist>First;teh>the");
    answer("Scond line⟦1⟧.", "Scond>Second");
    finish();
    store.acceptAll();
    expect(now().text).toBe(
      TEXT.replace("Frist line teh", "First line the").replace("Scond", "Second"),
    );
    // nothing was refused when it came to putting it in
    expect(store.state.kind).toBe("closed");
  });

  it("a run over one of them alone: nothing is sent, and that is what the bar is told", async () => {
    await opened(TEXT, SEGMENTS);
    for (const kind of ["paragraph", "selection"] as const) {
      selectBlock(1);
      await store.runOver(kind);
      expect(store.state, kind).toEqual({ kind: "nothing", locked: 1 });
    }
    selectBlock(3);
    await store.runHere();
    expect(store.state).toEqual({ kind: "nothing", locked: 1 });
    expect(runs).toEqual([]);
  });
});

describe("an HTML element written in a line, in the Edit-mode editor", () => {
  const TEXT = "Intro line.\n\nPress <kbd>Ctrl</kbd> to save teh file.\n";
  const SENT = "Press ⟦1⟧ to save teh file.";

  it("its text is never sent; fixes of the words around it go in, the element as written", async () => {
    await reviewOf(TEXT, blocks([1, 1], [3, 3]));
    expect(run.pieces.map((piece) => piece.text)).toEqual(["Intro line.", SENT]);
    expect(store.review?.locked).toBe(0);
    answer(SENT, "Press>Hit;teh>the");
    finish();
    store.acceptAll();
    expect(now().text).toBe(TEXT.replace("Press", "Hit").replace("teh", "the"));
    expect(store.state.kind).toBe("closed");
  });

  it("an answer for the whole paragraph (a translation) goes in: the element comes back as it was", async () => {
    await reviewOf(TEXT, blocks([1, 1], [3, 3]), WHOLE);
    answer(SENT, "Zum Speichern der Datei ⟦1⟧ drücken.");
    finish();
    expect(pending()).toHaveLength(1);
    store.accept();
    expect(now().text).toBe(
      TEXT.replace(/Press.*/, "Zum Speichern der Datei <kbd>Ctrl</kbd> drücken."),
    );
    expect(store.review?.outdated ?? 0).toBe(0);
    expect(store.state.kind).toBe("closed");
  });

  it("what the editor does not take: an answer that changes the element's own text", async () => {
    await reviewOf(TEXT, blocks([1, 1], [3, 3]));
    answer(SENT, "Ctrl>Strg");
    finish();
    store.accept();
    expect(now().text).toBe(TEXT);
    expect(store.review).toMatchObject({ outdated: 1 });
  });
});

describe("accept all in the Edit-mode editor", () => {
  const TEXT = "Frist line.\n\nSecond line teh.\n\nThird wrd here.\n\nLast lne.\n";

  it("an edit the editor refuses costs the others nothing: they go in together, and one undo takes them all back", async () => {
    await reviewOf(TEXT, blocks([1, 1], [3, 3], [5, 5], [7, 7]));
    answer("Frist line.", "Frist>First");
    // would turn its paragraph into a heading: the editor does not take an edit that changes what a block is
    answer("Second line teh.", "Second># Second");
    answer("Third wrd here.", "wrd>word");
    answer("Last lne.", "lne>line");
    finish();
    expect(pending()).toHaveLength(4);
    store.acceptAll();
    const accepted = TEXT.replace("Frist", "First").replace("wrd", "word").replace("lne", "line");
    expect(now().text).toBe(accepted);
    // the refused one is gone from the list and counted; the bar stays to say so
    expect(pending()).toEqual([]);
    expect(store.review).toMatchObject({ outdated: 1, total: 4 });
    const view = core.view;
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(now().text).toBe(TEXT);
    expect(now().changed).toBe(0);
    // nothing more to take back
    expect(undo(view.state, view.dispatch)).toBe(false);
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
