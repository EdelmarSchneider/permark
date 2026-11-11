// The translation review of a window against a fake editor: the languages a run is sent (none chosen: no run), each
// answer judged against them, one suggestion per piece that replaces exactly the piece, and the remembered targets.
// The review's own rules (news, accept | reject, offsets, marks) are driven in `plugins/correction/store.test.ts`.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import type { PieceRunPhase } from "../../api/generated/PieceRunPhase";
import type { TextEdit } from "../../core/editing/session.svelte";
import { checkEdits } from "../../core/editing/textEdits";
import type { Session } from "../local-ai/pieces/review.svelte";
import { Translation } from "./store.svelte";

type Fake = Session & { text: string; applied: TextEdit[][] };

/** An editor that holds a text and takes checked edits. */
function editor(initial: string): Fake {
  const fake: Fake = {
    path: "docs/a.md",
    mode: "source",
    state: { s: "loading" },
    text: initial,
    applied: [],
    current: () => fake.text,
    scope: (kind) => (kind === "selection" ? null : { from: 0, to: fake.text.length }),
    apply: (edits) => {
      const sorted = checkEdits(edits, fake.text.length, (f, t) => fake.text.slice(f, t));
      if (sorted === null) return false;
      fake.applied.push([...edits]);
      for (const e of [...sorted].reverse())
        fake.text = fake.text.slice(0, e.from) + e.insert + fake.text.slice(e.to);
      return true;
    },
    changeable: () => true,
    reveal: () => {},
    marks: () => {},
    markAt: () => null,
    composing: () => false,
    focus: () => {},
  };
  return fake;
}

const EN = "The weather is nice today.";
const PT = "O tempo está bom hoje.";
const DOC = `${EN}\n\n\`\`\`\ncode stays\n\`\`\`\n\nPerMark\n`;

let mock: ReturnType<typeof createMockApi>;
let session: Fake;
let store: Translation;

beforeEach(async () => {
  mock = createMockApi();
  mock.probe.translation.stepMs = 1e9;
  session = editor(DOC);
  store = new Translation(mock, session);
  await store.ask();
});

afterEach(() => {
  mock.probe.translation.stopAll();
});

const sent = () => mock.probe.translation.runs;

function news(phase: PieceRunPhase): void {
  const run = sent().at(-1)?.run;
  if (run === undefined) throw new Error("no run sent");
  store.news({ run, phase });
}

function answer(id: number, text: string): void {
  news({ kind: "piece", id, result: { kind: "output", text } });
}

describe("a translation run", () => {
  it("does not start while no language to translate into is chosen", async () => {
    expect(store.available).toBe(true);
    await store.runOver("document");
    await store.runHere();
    expect(sent()).toEqual([]);
    expect(store.state.kind).toBe("closed");
  });

  it("carries the target, and the source when the person named it", async () => {
    await store.languages.choose("pt-BR");
    await store.runOver("document");
    expect(sent()).toEqual([
      {
        run: sent()[0]?.run,
        pieces: [
          { id: 0, text: EN },
          { id: 1, text: "PerMark" },
        ],
        target: { tag: "pt-BR", name: "Brazilian Portuguese" },
        source: null,
      },
    ]);
    store.languages.from("en-US");
    await store.runOver("document");
    expect(sent()[1]).toMatchObject({
      target: { tag: "pt-BR", name: "Brazilian Portuguese" },
      source: { tag: "en-US", name: "American English" },
    });
  });

  it("an answer is one suggestion for its whole piece; accepting it replaces exactly the piece", async () => {
    await store.languages.choose("pt-BR");
    await store.runOver("document");
    answer(0, PT);
    expect(store.review?.suggestions).toMatchObject([
      { from: 0, to: EN.length, original: EN, insert: PT },
    ]);
    news({ kind: "done" });
    store.accept();
    expect(session.applied).toEqual([[{ from: 0, to: EN.length, insert: PT, expect: EN }]]);
    expect(session.text).toBe(DOC.replace(EN, PT));
    expect(store.state.kind).toBe("closed");
  });

  it("an answer that says the same is counted apart from one that is no translation", async () => {
    await store.languages.choose("pt-BR");
    await store.runOver("document");
    answer(1, "PerMark");
    answer(0, `${PT}\n\nNote: I kept the tone.`);
    news({ kind: "done" });
    expect(store.review).toMatchObject({ suggestions: [], same: 1, discarded: 1 });
  });

  it("answers are judged against the languages their run was sent, whatever is chosen since", async () => {
    await store.languages.choose("ja");
    await store.runOver("document");
    await store.languages.choose("pt-BR");
    answer(0, PT);
    expect(store.review).toMatchObject({ suggestions: [], discarded: 1 });
    await store.runOver("document");
    answer(0, PT);
    expect(store.review?.suggestions).toHaveLength(1);
  });

  it("is dormant while no model is assigned to the job", async () => {
    await store.languages.choose("pt-BR");
    await mock.pluginCall("local-ai.assign", { task: "translation", model: null });
    await store.ask();
    expect(store.available).toBe(false);
    await store.runOver("document");
    expect(sent()).toEqual([]);
    await mock.pluginCall("local-ai.assign", { task: "translation", model: "gemma-4-e2b-it-q4_0" });
    await store.ask();
    expect(store.available).toBe(true);
    await store.runOver("document");
    expect(sent()).toHaveLength(1);
  });
});

describe("the languages of a window", () => {
  it("come from what the backend remembers", async () => {
    const languages = store.languages;
    expect(languages).toMatchObject({ target: null, source: null, recent: [] });
    expect(languages.runArgs()).toBeNull();
    mock.probe.translation.prefs = { target: "de", recent: ["de", "ja"] };
    await languages.load();
    expect(languages).toMatchObject({ target: "de", recent: ["de", "ja"] });
    expect(languages.runArgs()).toEqual({ target: { tag: "de", name: "German" }, source: null });
  });

  it("a chosen target holds at once and is remembered, newest first", async () => {
    const languages = store.languages;
    mock.probe.translation.prefs = { target: "de", recent: ["de", "ja"] };
    const chosen = languages.choose("pt-BR");
    expect(languages.target).toBe("pt-BR");
    await chosen;
    expect(languages).toMatchObject({ target: "pt-BR", recent: ["pt-BR", "de", "ja"] });
    expect(mock.probe.translation.prefs).toEqual({
      target: "pt-BR",
      recent: ["pt-BR", "de", "ja"],
    });
  });

  it("an answer that comes late does not undo a newer choice", async () => {
    const languages = store.languages;
    const first = languages.choose("de");
    const second = languages.choose("ja");
    await Promise.all([first, second]);
    expect(languages).toMatchObject({ target: "ja", recent: ["ja", "de"] });
  });

  it("a language the prompt cannot name is not taken, chosen or remembered", async () => {
    const languages = store.languages;
    await languages.choose("qaa");
    expect(languages.target).toBeNull();
    expect(mock.probe.translation.prefs.target).toBeNull();
    mock.probe.translation.prefs = { target: "qaa", recent: ["qaa", "de"] };
    await languages.load();
    expect(languages).toMatchObject({ target: null, recent: ["de"] });
    languages.from("qaa");
    expect(languages.source).toBeNull();
  });

  it("the source is never the target", async () => {
    const languages = store.languages;
    await languages.choose("pt-BR");
    languages.from("PT-br");
    expect(languages.source).toBeNull();
    languages.from("en-US");
    expect(languages.source).toBe("en-US");
    await languages.choose("en-US");
    expect(languages).toMatchObject({ target: "en-US", source: null });
    languages.from("de");
    languages.from(null);
    expect(languages.source).toBeNull();
  });

  it("reset forgets them", async () => {
    const languages = store.languages;
    await languages.choose("pt-BR");
    languages.from("en-US");
    languages.reset();
    expect(languages).toMatchObject({ target: null, source: null, recent: [] });
  });
});
