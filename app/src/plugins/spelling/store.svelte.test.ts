// @vitest-environment jsdom
// The spelling marks of a window over a fake editor and the mock backend (English ready with a small word list).
import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import type { EditMode, TextEdit, TextRange } from "../../core/editing/session.svelte";
import type { MarkOptions } from "../../core/editing/marks";
import { PluginClient } from "../../core/plugins/client";
import { BATCH, LAYER, SpellingStore, type SpellingSession } from "./store.svelte";

type Drawn = { ranges: TextRange[]; layer: string; options: MarkOptions };

/** An editor holding a text; the caret is an offset (`null`: elsewhere). */
class FakeSession implements SpellingSession {
  path = $state<string | null>("docs/a.md");
  mode = $state<EditMode>("source");
  text = $state.raw("");
  caret: number | null = null;
  composingNow = false;
  native = true;
  drawn: Drawn[] = [];
  applied: TextEdit[][] = [];

  current = (): string => this.text;
  apply = (edits: readonly TextEdit[]): boolean => {
    if (this.composingNow) return false;
    this.applied.push([...edits]);
    return true;
  };
  marks = (
    ranges: readonly TextRange[],
    _current: number | null,
    _pick: (index: number) => void,
    layer = "correction",
    options: MarkOptions = {},
  ): void => {
    this.drawn.push({ ranges: [...ranges], layer, options });
  };
  markAt = (layer = "correction"): number | null => {
    const last = this.last(layer);
    const caret = this.caret;
    if (caret === null || last === undefined) return null;
    const at = last.ranges.findIndex((r) => r.from <= caret && caret <= r.to);
    return at < 0 ? null : at;
  };
  composing = (): boolean => this.composingNow;
  setNativeSpellcheck = (on: boolean): void => {
    this.native = on;
  };

  last(layer = LAYER): Drawn | undefined {
    return this.drawn.filter((d) => d.layer === layer).at(-1);
  }

  /** The words marked now on the spelling layer. */
  marked(): string[] {
    return (this.last()?.ranges ?? []).map((r) => this.text.slice(r.from, r.to));
  }
}

let mock: ReturnType<typeof createMockApi>;
let session: FakeSession;
/** Whether an AI review holds the editor. */
class Review {
  open = $state(false);
}

let review: Review;
let store: SpellingStore;
let stop: () => void;

beforeEach(() => {
  vi.useFakeTimers();
  mock = createMockApi();
  session = new FakeSession();
  review = new Review();
  store = new SpellingStore({
    api: mock,
    client: new PluginClient(mock),
    session,
    reviewOpen: () => review.open,
    uiLocale: () => "en",
  });
});

afterEach(() => {
  stop?.();
  vi.useRealTimers();
});

/** Starts the store over `text` and lets the first pass end. */
async function startWith(text: string): Promise<void> {
  session.text = text;
  stop = store.start();
  await settle();
}

/** Effects run, the pause passes, the backend answers. */
async function settle(): Promise<void> {
  flushSync();
  await vi.advanceTimersByTimeAsync(1000);
}

const checks = (): string[] => mock.probe.spelling.calls.filter((c) => c.startsWith("check"));

describe("marks", () => {
  it("only the misspelled words, on the spelling layer with the wavy look", async () => {
    await startWith("Hello wrold, this `cdoe` is a documnet.");
    expect(session.marked()).toEqual(["wrold", "documnet"]);
    expect(session.last()?.options.className).toBe("wavy-mark");
    expect(session.drawn.every((d) => d.layer === LAYER)).toBe(true);
    expect(store.misspelled.map((w) => w.word)).toEqual(["wrold", "documnet"]);
  });

  it("of a hyphenated word unknown as a whole, the unknown parts", async () => {
    await startWith("A hello-wrold text");
    expect(session.marked()).toEqual(["wrold"]);
  });

  it("the cache sends only words not seen yet", async () => {
    await startWith("Hello wrold");
    expect(checks()).toEqual(["check 2"]);
    session.text = "Hello wrold Hello";
    await settle();
    expect(checks()).toEqual(["check 2"]);
    session.text = "Hello wrold Hello nwe";
    await settle();
    expect(checks()).toEqual(["check 2", "check 1"]);
  });

  it("batches of at most the backend's limit", async () => {
    const many = Array.from(
      { length: BATCH + 10 },
      (_, i) => `w${"abcdefghij"[i % 10]}${toLetters(i)}`,
    );
    await startWith(many.join(" "));
    expect(checks()).toEqual([`check ${BATCH}`, "check 10"]);
  });

  it("not the word at the caret while typing; marked once the person types on", async () => {
    await startWith("Hello");
    session.text = "Hello wrold";
    session.caret = 11;
    await settle();
    expect(session.marked()).toEqual([]);
    session.text = "Hello wrold text";
    session.caret = 16;
    await settle();
    expect(session.marked()).toEqual(["wrold"]);
  });

  it("nothing while text is composed; the pass waits for the composition to end", async () => {
    session.composingNow = true;
    await startWith("Hello wrold");
    expect(session.marked()).toEqual([]);
    session.composingNow = false;
    await settle();
    expect(session.marked()).toEqual(["wrold"]);
  });

  it("an AI review open clears the layer; closed, the marks come back", async () => {
    await startWith("Hello wrold");
    expect(session.marked()).toEqual(["wrold"]);
    review.open = true;
    await settle();
    expect(session.marked()).toEqual([]);
    review.open = false;
    await settle();
    expect(session.marked()).toEqual(["wrold"]);
  });

  it("new personal words: the cache is asked again", async () => {
    await startWith("Hello wrold");
    expect(await store.addToDictionary("wrold")).toBe(true);
    await settle();
    expect(session.marked()).toEqual([]);
    expect(checks().length).toBe(2);
  });

  it("an ignored word: unmarked in this document only", async () => {
    await startWith("Hello wrold documnet");
    store.ignore("wrold");
    await settle();
    expect(session.marked()).toEqual(["documnet"]);
    session.path = "docs/b.md";
    await settle();
    expect(session.marked()).toEqual(["wrold", "documnet"]);
  });

  it("a text in a script no dictionary covers: no marks", async () => {
    await startWith("Привет мир это текст на русском языке для проверки");
    expect(store.document?.language.source).toBe("unsupported");
    expect(session.marked()).toEqual([]);
    expect(checks()).toEqual([]);
  });

  it("no dictionary ready for the document's language: no marks, the language to download said", async () => {
    mock.probe.spelling.declared.set("docs/a.md", "de");
    await startWith("Hallo Welt");
    expect(session.marked()).toEqual([]);
    expect(store.needsDownload).toEqual(["de"]);
    expect(checks()).toEqual([]);
    // the dictionary arrives: the marks follow
    mock.probe.spelling.setState("de", { kind: "ready" });
    await settle();
    expect(store.needsDownload).toEqual([]);
    expect(checks().length).toBe(1);
  });

  it("the language is asked once per open document", async () => {
    const asked = vi.spyOn(mock, "pluginCall");
    await startWith("Hello wrold");
    session.text = "Hello wrold nwe";
    await settle();
    const languageCalls = asked.mock.calls.filter(([method]) => method === "spelling.language");
    expect(languageCalls.length).toBe(1);
  });

  it("View: no marks", async () => {
    await startWith("Hello wrold");
    session.mode = "view";
    await settle();
    expect(session.marked()).toEqual([]);
  });
});

describe("the word menu", () => {
  it("a right-click on a mark opens it; replace puts the word in as one edit", async () => {
    await startWith("Hello wrold");
    session.last()?.options.menu?.(0, 10, 20);
    expect(store.menuAt).toEqual({ index: 0, word: "wrold", x: 10, y: 20 });
    expect(await store.suggestions("wrold")).toContain("world");
    expect(store.replace(0, "world")).toBe(true);
    expect(session.applied).toEqual([[{ from: 6, to: 11, insert: "world", expect: "wrold" }]]);
    expect(store.menuAt).toBeNull();
  });

  it("replace is refused while composing", async () => {
    await startWith("Hello wrold");
    session.composingNow = true;
    expect(store.replace(0, "world")).toBe(false);
  });
});

describe("on and off", () => {
  it("on: the webview's spellcheck off; off: back on, the layer cleared, nothing heard", async () => {
    await startWith("Hello wrold");
    expect(session.native).toBe(false);
    stop();
    expect(session.native).toBe(true);
    expect(session.marked()).toEqual([]);
    const drawn = session.drawn.length;
    session.text = "Hello wrold nwe";
    mock.probe.spelling.setState("de", { kind: "ready" });
    await settle();
    expect(session.drawn.length).toBe(drawn);
  });
});

/** `n` as letters only (the tokenizer leaves words with digits out). */
function toLetters(n: number): string {
  let out = "";
  let rest = n;
  do {
    out += "abcdefghijklmnopqrstuvwxyz"[rest % 26] ?? "";
    rest = Math.floor(rest / 26);
  } while (rest > 0);
  return out;
}
