// @vitest-environment jsdom
// The piece review's own rules, whatever plugin configures it: the changed range of a text, an answer that says the
// same as its piece, pieces the editor on screen cannot change (never sent, counted), one review at a time in an
// editor (jsdom: effects run, so what reads `blocked` is heard). A
// whole review (runs, news, accept | reject, marks) is driven where a plugin configures it:
// `plugins/correction/store.test.ts`; on the Edit-mode editor: `reviewEdit.test.ts`.
import { flushSync } from "svelte";
import { beforeEach, describe, expect, it } from "vitest";
import { createMockApi } from "../../../api/mock";
import { observe } from "../../../core/testing/effect.svelte";
import {
  changedRange,
  openReview,
  PieceReview,
  type ReviewConfig,
  type Session,
} from "./review.svelte";
import { pieces } from "./segment";

describe("changed ranges", () => {
  it("prefix and suffix kept, the rest is the change", () => {
    expect(changedRange("abcdef", "abXdef")).toEqual({ from: 2, to: 3, delta: 0 });
    expect(changedRange("abc", "abcd")).toEqual({ from: 3, to: 3, delta: 1 });
    expect(changedRange("aaaa", "aa")).toEqual({ from: 2, to: 4, delta: -2 });
  });
});

const DOC = "Already fine.\n\nNot an answer.\n\nAlso fine.\n";

/** An editor that holds `text` and takes no edit. */
function holding(text: string): Session {
  return {
    path: "docs/a.md",
    mode: "source",
    state: { s: "loading" },
    text,
    current: () => text,
    scope: () => ({ from: 0, to: text.length }),
    apply: () => false,
    changeable: () => true,
    reveal: () => {},
    marks: () => {},
    markAt: () => null,
    composing: () => false,
    focus: () => {},
  };
}

/** A plugin for which an answer equal to its piece says the same, any other is no answer at all. */
const CONFIG: ReviewConfig = {
  plugin: "correction",
  prepare: pieces,
  judge: (_, piece, output) =>
    output === piece.text ? { kind: "same" } : { kind: "discard", reason: "other" },
  runArgs: () => ({}),
};

let mock: ReturnType<typeof createMockApi>;

beforeEach(() => {
  mock = createMockApi();
  mock.probe.correction.stepMs = 1e9;
});

async function review(session: Session): Promise<PieceReview> {
  const store = new PieceReview(CONFIG, mock, session);
  await store.ask();
  return store;
}

const runs = (): number => mock.probe.correction.runs.length;

describe("an answer that says the same as its piece", () => {
  it("is counted on its own: no suggestion, not a piece left as it was", async () => {
    const store = await review(holding(DOC));
    await store.runOver("document");
    const sent = mock.probe.correction.runs.at(-1);
    if (!sent) throw new Error("no run sent");
    for (const { id, text } of sent.pieces) {
      const output = text === "Not an answer." ? "Sure!" : text;
      store.news({
        run: sent.run,
        phase: { kind: "piece", id, result: { kind: "output", text: output } },
      });
    }
    store.news({ run: sent.run, phase: { kind: "done" } });
    expect(store.review).toMatchObject({
      suggestions: [],
      total: 0,
      same: 2,
      discarded: 1,
      outdated: 0,
      locked: 0,
    });
  });
});

describe("pieces the editor on screen cannot change", () => {
  /** An editor that takes no edit where the text reads `kept`. */
  const keeping = (...kept: string[]): Session => ({
    ...holding(DOC),
    changeable: ({ from, to }) => !kept.includes(DOC.slice(from, to)),
  });

  it("are not sent (their answer could not go in) and counted for the bar", async () => {
    const store = await review(keeping("Not an answer."));
    await store.runOver("document");
    expect(mock.probe.correction.runs.at(-1)?.pieces.map((piece) => piece.text)).toEqual([
      "Already fine.",
      "Also fine.",
    ]);
    expect(store.review).toMatchObject({ locked: 1, suggestions: [] });
  });

  it("a scope that holds nothing else: no run, and the bar is told how many instead of 'nothing here'", async () => {
    const session = keeping("Already fine.", "Not an answer.", "Also fine.");
    const store = await review(session);
    const other = await review(session);
    await store.runOver("document");
    expect(runs()).toBe(0);
    expect(store.state).toEqual({ kind: "nothing", locked: 3 });
    // nothing to go through: the editor is not held
    expect(other.blocked).toBe(false);
    // a scope with no text at all says so as ever
    const empty = await review(holding("```\ncode\n```\n"));
    await empty.runOver("document");
    expect(empty.state).toEqual({ kind: "nothing", locked: 0 });
  });
});

describe("one review at a time in an editor", () => {
  it("while one is open another does not run and says it is blocked; the open one runs again as ever", async () => {
    const session = holding(DOC);
    const first = await review(session);
    const second = await review(session);
    expect([first.blocked, second.blocked]).toEqual([false, false]);
    expect(openReview(session)).toBeNull();
    await first.runOver("document");
    expect([first.blocked, second.blocked]).toEqual([false, true]);
    // the one open in this editor: what the keys for the next | previous suggestion act on
    expect(openReview(session)).toBe(first);
    await second.runOver("document");
    await second.runHere();
    expect(second.state.kind).toBe("closed");
    expect(runs()).toBe(1);
    await first.runOver("paragraph");
    expect(runs()).toBe(2);
    // an editor of its own (another window in the tests) is not held
    const elsewhere = await review(holding(DOC));
    expect(elsewhere.blocked).toBe(false);
  });

  it("a stopped run keeps its review open; Done, View and the plugin going off free the editor", async () => {
    const session = holding(DOC);
    const first = await review(session);
    const second = await review(session);
    await first.runOver("document");
    first.cancel();
    expect(first.review?.status.kind).toBe("cancelled");
    expect(second.blocked).toBe(true);
    first.done();
    expect(second.blocked).toBe(false);

    await second.runOver("document");
    expect(second.review).not.toBeNull();
    expect(first.blocked).toBe(true);
    second.left("docs/a.md", "view", false);
    expect(first.blocked).toBe(false);

    const off = first.start();
    await first.runOver("document");
    expect(second.blocked).toBe(true);
    off();
    expect(second.blocked).toBe(false);
  });

  it("a bar that only says there was nothing to send holds nothing", async () => {
    const session = holding("```\ncode\n```\n");
    const first = await review(session);
    const second = await review(session);
    await first.runOver("document");
    expect(first.state.kind).toBe("nothing");
    expect(second.blocked).toBe(false);
  });

  it("what reads `blocked` hears when it changes", async () => {
    const session = holding(DOC);
    const first = await review(session);
    const second = await review(session);
    const seen: boolean[] = [];
    const watch = observe(() => void seen.push(second.blocked));
    flushSync();
    await first.runOver("document");
    flushSync();
    first.done();
    flushSync();
    watch.stop();
    expect(seen).toEqual([false, true, false]);
  });
});
