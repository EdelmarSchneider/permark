// The correction review of a window against a fake editor: what a run sends, news of other runs ignored, guards on
// each answer, suggestions in document offsets, accept | reject (one by one, all at once = one edit), offsets that
// follow accepted edits and the person's typing, IME composition, cancel, and what closes the review.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import type { CorrectionPhase } from "../../api/generated/CorrectionPhase";
import type { EditScope, TextEdit, TextRange } from "../../core/editing/session.svelte";
import { checkEdits } from "../../core/editing/textEdits";
import { changedRange, CorrectionStore, type Session } from "./store.svelte";

type Fake = Session & {
  text: string;
  composingNow: boolean;
  refuse: boolean;
  selection: TextRange | null;
  applied: TextEdit[][];
  revealed: TextRange[];
  focused: number;
  type(at: number, insert: string, remove?: number): void;
};

function fakeSession(initial: string): Fake {
  const fake: Fake = {
    path: "docs/a.md",
    mode: "source",
    state: { s: "loading" },
    text: initial,
    composingNow: false,
    refuse: false,
    selection: null,
    applied: [],
    revealed: [],
    focused: 0,
    current: () => fake.text,
    scope: (kind: EditScope) => {
      if (kind === "document") return { from: 0, to: fake.text.length };
      if (kind === "selection") return fake.selection;
      return { from: 0, to: fake.text.indexOf("\n\n") };
    },
    apply: (edits) => {
      if (fake.composingNow || fake.refuse) return false;
      const sorted = checkEdits(edits, fake.text.length, (f, t) => fake.text.slice(f, t));
      if (sorted === null) return false;
      fake.applied.push([...edits]);
      for (const e of [...sorted].reverse())
        fake.text = fake.text.slice(0, e.from) + e.insert + fake.text.slice(e.to);
      return true;
    },
    reveal: (range) => void fake.revealed.push(range),
    composing: () => fake.composingNow,
    focus: () => void (fake.focused += 1),
    type: (at, insert, remove = 0) => {
      fake.text = fake.text.slice(0, at) + insert + fake.text.slice(at + remove);
    },
  };
  return fake;
}

const DOC = "I has a cat.\n\n```\nteh code\n```\n\nShe recieve mail from teh office.\n";

let mock: ReturnType<typeof createMockApi>;
let session: Fake;
let store: CorrectionStore;
let sent: { run: string; pieces: { id: number; text: string }[] }[] = [];

beforeEach(async () => {
  mock = createMockApi();
  mock.probe.correction.stepMs = 1e9;
  sent = mock.probe.correction.runs;
  session = fakeSession(DOC);
  store = new CorrectionStore(mock, session);
  await store.ask();
});

afterEach(() => {
  mock.probe.correction.stopAll();
  vi.useRealTimers();
});

function runId(): string {
  const last = sent.at(-1);
  if (!last) throw new Error("no run sent");
  return last.run;
}

function news(phase: CorrectionPhase, run = runId()): void {
  store.news({ run, phase });
}

/** The model's answer for piece `id`: its text with `fix` applied. */
function answer(id: number, fix: (text: string) => string): void {
  const piece = sent.at(-1)?.pieces.find((p) => p.id === id);
  if (!piece) throw new Error(`no piece ${id}`);
  news({ kind: "piece", id, result: { kind: "output", text: fix(piece.text) } });
}

const suggestions = () =>
  (store.review?.suggestions ?? []).map((s) => `${s.from}-${s.to}:${s.original}>${s.insert}`);

/** Runs over the whole DOC with both prose pieces corrected. */
async function reviewed(): Promise<void> {
  await store.correct("document");
  answer(0, (t) => t.replace("has", "have"));
  answer(1, (t) => t.replace("recieve", "receives").replace("teh", "the"));
  news({ kind: "done" });
}

describe("a correction run", () => {
  it("sends only the prose pieces of the scope, the caret's block first", async () => {
    await store.correct("document");
    expect(sent).toHaveLength(1);
    expect(sent[0]?.pieces.map((p) => p.text)).toEqual([
      "I has a cat.",
      "She recieve mail from teh office.",
    ]);
    expect(store.review?.status).toEqual({ kind: "loading", progress: 0 });
    session.selection = { from: 32, to: 65 };
    await store.correct("selection");
    expect(sent[1]?.pieces.map((p) => p.text)).toEqual(["She recieve mail from teh office."]);
    // the earlier run was stopped
    expect(mock.probe.correction.calls).toContain(`cancel ${sent[0]?.run}`);
  });

  it("nothing to check: no run, the bar says so", async () => {
    session.text = "```\nteh\n```\n";
    await store.correct("document");
    expect(sent).toHaveLength(0);
    expect(store.state.kind).toBe("nothing");
  });

  it("is dormant while no model is assigned", async () => {
    mock.probe.correction.available = false;
    await store.ask();
    expect(store.available).toBe(false);
    await store.correct("document");
    expect(sent).toHaveLength(0);
    expect(store.state.kind).toBe("closed");
  });

  it("hears only its own run; progress and parts follow the news", async () => {
    await store.correct("document");
    news({ kind: "loading", progress: 0.5 });
    expect(store.review?.status).toEqual({ kind: "loading", progress: 0.5 });
    news({ kind: "loading", progress: 0.9 }, "another-window");
    expect(store.review?.status).toEqual({ kind: "loading", progress: 0.5 });
    answer(0, (t) => t.replace("has", "have"));
    expect(store.review?.status).toEqual({ kind: "checking", part: 2, parts: 2 });
    expect(suggestions()).toEqual(["2-5:has>have"]);
    news({ kind: "piece", id: 1, result: { kind: "output", text: "x" } }, "another-window");
    expect(suggestions()).toEqual(["2-5:has>have"]);
  });

  it("guards each answer: too much changed | dropped by the plugin -> counted, never shown", async () => {
    await store.correct("document");
    answer(0, () => "Completely different words were written here instead.");
    news({ kind: "piece", id: 1, result: { kind: "discarded", reason: "tooLong" } });
    news({ kind: "done" });
    expect(store.review?.suggestions).toEqual([]);
    expect(store.review?.discarded).toBe(2);
    expect(store.review?.status.kind).toBe("finished");
  });

  it("suggestions come in document order with document offsets", async () => {
    await store.correct("document");
    answer(1, (t) => t.replace("recieve", "receives").replace("teh", "the"));
    answer(0, (t) => t.replace("has", "have"));
    expect(suggestions()).toEqual(["2-5:has>have", "36-43:recieve>receives", "54-57:teh>the"]);
  });

  it("an answer for a piece the person changed meanwhile is dropped", async () => {
    await store.correct("document");
    session.type(0, "Now ");
    answer(0, (t) => t.replace("has", "have"));
    expect(suggestions()).toEqual([]);
  });

  it("failure: the error is kept for the bar", async () => {
    await store.correct("document");
    news({
      kind: "failed",
      error: { kind: "unavailable", subject: "model", detail: "noModel" },
    });
    expect(store.review?.status).toEqual({
      kind: "failed",
      error: {
        kind: "backend",
        command: "plugin_call",
        error: { kind: "unavailable", subject: "model", detail: "noModel" },
      },
    });
  });
});

describe("the review", () => {
  it("accept puts the change in and moves the later suggestions by its length", async () => {
    await reviewed();
    store.accept();
    expect(session.applied).toEqual([[{ from: 2, to: 5, insert: "have", expect: "has" }]]);
    expect(session.text.startsWith("I have a cat.")).toBe(true);
    expect(suggestions()).toEqual(["37-44:recieve>receives", "55-58:teh>the"]);
    store.accept();
    // the excerpt reads the text as it is now
    expect(store.review?.suggestions[0]?.before).toContain("She receives mail from ");
    store.accept();
    expect(session.text).toBe(
      "I have a cat.\n\n```\nteh code\n```\n\nShe receives mail from the office.\n",
    );
    // the last one done: the review closes, the keyboard back to the editor
    expect(store.state.kind).toBe("closed");
    expect(session.focused).toBe(1);
  });

  it("reject leaves the text; next | previous wrap and show the suggestion", async () => {
    await reviewed();
    store.go(-1);
    expect(store.review?.at).toBe(2);
    expect(session.revealed.at(-1)).toEqual({ from: 54, to: 57 });
    store.reject();
    expect(session.text).toBe(DOC);
    expect(suggestions()).toEqual(["2-5:has>have", "36-43:recieve>receives"]);
    expect(store.review?.at).toBe(1);
  });

  it("accept all = one edit (one undo step); reject all leaves everything", async () => {
    await reviewed();
    store.acceptAll();
    expect(session.applied).toHaveLength(1);
    expect(session.applied[0]).toHaveLength(3);
    expect(session.text).toBe(
      "I have a cat.\n\n```\nteh code\n```\n\nShe receives mail from the office.\n",
    );
    expect(store.state.kind).toBe("closed");
    session.text = DOC;
    await reviewed();
    expect(suggestions()).toHaveLength(3);
    store.rejectAll();
    expect(session.applied).toHaveLength(1);
    expect(store.state.kind).toBe("closed");
  });

  it("accept all | reject all wait for the run to end", async () => {
    await store.correct("document");
    answer(0, (t) => t.replace("has", "have"));
    store.acceptAll();
    store.rejectAll();
    expect(session.applied).toEqual([]);
    expect(suggestions()).toHaveLength(1);
  });

  it("typing drops the suggestions it touches and shifts the rest", async () => {
    await reviewed();
    session.type(2, "x");
    store.sync();
    expect(suggestions()).toEqual(["37-44:recieve>receives", "55-58:teh>the"]);
    expect(store.review?.outdated).toBe(1);
    store.accept();
    expect(session.text).toContain("She receives mail from teh office.");
  });

  it("an edit the editor refuses is dropped, never applied blind", async () => {
    await reviewed();
    session.refuse = true;
    store.accept();
    expect(suggestions()).toHaveLength(2);
    expect(store.review?.outdated).toBe(1);
    expect(session.text).toBe(DOC);
  });

  it("while text is composed (IME) the accept waits, then goes in", async () => {
    vi.useFakeTimers();
    await reviewed();
    session.composingNow = true;
    store.accept();
    expect(session.applied).toEqual([]);
    await vi.advanceTimersByTimeAsync(350);
    expect(session.applied).toEqual([]);
    session.composingNow = false;
    await vi.advanceTimersByTimeAsync(150);
    expect(session.applied).toHaveLength(1);
    expect(session.text.startsWith("I have")).toBe(true);
  });

  it("cancel stops the run and keeps what was found", async () => {
    await store.correct("document");
    answer(0, (t) => t.replace("has", "have"));
    const run = runId();
    store.cancel();
    expect(mock.probe.correction.calls).toContain(`cancel ${run}`);
    expect(store.review?.status.kind).toBe("cancelled");
    answer(1, (t) => t.replace("teh", "the"));
    expect(suggestions()).toEqual(["2-5:has>have"]);
  });

  it("another file, a conflict or View closes the review (and stops its run)", async () => {
    for (const [path, mode, conflict] of [
      ["docs/b.md", "source", false],
      ["docs/a.md", "view", false],
      ["docs/a.md", "edit", true],
    ] as const) {
      await store.correct("document");
      store.left("docs/a.md", "edit", false);
      expect(store.state.kind).toBe("review");
      store.left(path, mode, conflict);
      expect(store.state.kind).toBe("closed");
      expect(mock.probe.correction.calls).toContain(`cancel ${runId()}`);
    }
  });

  it("Mod+Alt+K in review asks the bar for the keyboard", async () => {
    await reviewed();
    const before = store.focusRequest;
    await store.correctHere();
    expect(store.focusRequest).toBe(before + 1);
    expect(sent).toHaveLength(1);
  });
});

describe("changed ranges", () => {
  it("prefix and suffix kept, the rest is the change", () => {
    expect(changedRange("abcdef", "abXdef")).toEqual({ from: 2, to: 3, delta: 0 });
    expect(changedRange("abc", "abcd")).toEqual({ from: 3, to: 3, delta: 1 });
    expect(changedRange("aaaa", "aa")).toEqual({ from: 2, to: 4, delta: -2 });
  });
});
