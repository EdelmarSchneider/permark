// @vitest-environment jsdom
// The spelling marks of a window over a fake editor and the mock backend (English ready with a small word list),
// and what the person sees of them: a marked word's menu (mouse and keyboard), the step to the next misspelling, the
// status bar item with its language picker and download question, the settings.
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import type { EditMode, EditScope, TextEdit, TextRange } from "../../core/editing/session.svelte";
import type { MarkOptions } from "../../core/editing/marks";
import type { MenuEntry } from "../../core/explorer/menu";
import { PluginClient } from "../../core/plugins/client";
import type { NoticeAction } from "../../core/window.svelte";
import { defaultEntries, languageEntries, wordEntries } from "./menus";
import SpellingSettings from "./SpellingSettings.svelte";
import SpellingStatus from "./SpellingStatus.svelte";
import { BATCH, CARET_MS, LAYER, SpellingStore, type SpellingSession } from "./store.svelte";

type Drawn = { ranges: TextRange[]; layer: string; options: MarkOptions };

/** An editor holding a text; the caret is an offset (`null`: elsewhere). */
class FakeSession implements SpellingSession {
  path = $state<string | null>("docs/a.md");
  mode = $state<EditMode>("source");
  text = $state.raw("");
  caret: number | null = null;
  /** What `scope` answers per kind (`null`: none). */
  scopes: Record<EditScope, TextRange | null> = {
    selection: null,
    paragraph: null,
    document: null,
  };
  composingNow = false;
  native = true;
  drawn: Drawn[] = [];
  applied: TextEdit[][] = [];
  revealed: TextRange[] = [];
  focused = 0;

  reveal = (range: TextRange): void => {
    this.revealed.push(range);
    this.caret = range.to;
  };
  scope = (kind: EditScope): TextRange | null => this.scopes[kind];
  focus = (): void => {
    this.focused += 1;
  };

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
let said: string[];
let notices: { key: string; text: string; actions: NoticeAction[] }[];

beforeEach(() => {
  vi.useFakeTimers();
  mock = createMockApi();
  session = new FakeSession();
  review = new Review();
  said = [];
  notices = [];
  store = new SpellingStore({
    api: mock,
    client: new PluginClient(mock),
    session,
    reviewOpen: () => review.open,
    uiLocale: () => "en",
    say: (text) => said.push(text),
    notify: (key, text, ...actions) => notices.push({ key, text, actions }),
  });
});

afterEach(() => {
  stop?.();
  vi.useRealTimers();
  document.body.replaceChildren();
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

  it("its corrections are asked when it opens", async () => {
    await startWith("Hello wrold");
    session.last()?.options.menu?.(0, 10, 20);
    expect(store.suggested).toBeNull();
    await settle();
    expect(store.suggested?.word).toBe("wrold");
    expect(store.suggested?.list).toContain("world");
  });

  it("corrections first, then Add to dictionary, Ignore in this document and the language", async () => {
    await startWith("Hello wrold");
    const at = { index: 0, word: "wrold", x: 1, y: 2 };
    expect(labels(wordEntries(store, at, ["world", "would"]))).toEqual([
      "world",
      "would",
      "Add to dictionary",
      "Ignore in this document",
      "Language: English…",
    ]);
    const none = wordEntries(store, at, []);
    expect(none[0]).toMatchObject({ label: "No suggestions", disabled: true });
    run(wordEntries(store, at, ["world"]), "world");
    expect(session.applied).toEqual([[{ from: 6, to: 11, insert: "world", expect: "wrold" }]]);
  });

  it("Add to dictionary and Ignore from the menu: the mark goes", async () => {
    await startWith("Hello wrold documnet");
    run(
      wordEntries(store, { index: 1, word: "documnet", x: 0, y: 0 }, []),
      "Ignore in this document",
    );
    await settle();
    expect(session.marked()).toEqual(["wrold"]);
    run(wordEntries(store, { index: 0, word: "wrold", x: 0, y: 0 }, []), "Add to dictionary");
    await settle();
    expect(session.marked()).toEqual([]);
    expect(store.personal).toEqual(["wrold"]);
  });

  it("Shift+F10 | the Menu key in the editor: the menu of the word at the caret, where the browser aims it", async () => {
    await startWith("Hello wrold");
    const editor = editorElement();
    session.caret = 8;
    const event = contextMenu(editor, 0);
    expect(event.defaultPrevented).toBe(true);
    expect(store.menuAt).toEqual({ index: 0, word: "wrold", x: 30, y: 40 });
  });

  it("the webview's own menu stays for a right-click off a mark, a caret on no mark, a field", async () => {
    await startWith("Hello wrold");
    const editor = editorElement();
    session.caret = 8;
    // the pointer: the editor opens the menu itself when it lands on a mark
    expect(contextMenu(editor, 2).defaultPrevented).toBe(false);
    const field = document.createElement("input");
    document.body.append(field);
    expect(contextMenu(field, 0).defaultPrevented).toBe(false);
    session.caret = 2;
    expect(contextMenu(editor, 0).defaultPrevented).toBe(false);
    expect(store.menuAt).toBeNull();
  });
});

describe("the next misspelling", () => {
  it("from the mark at the caret to the next, around the end; the word said with its first correction", async () => {
    await startWith("Hello wrold and documnet");
    session.caret = 7;
    expect(store.canStep).toBe(true);
    expect(await store.next()).toBe(true);
    expect(session.revealed.at(-1)).toEqual({ from: 16, to: 24 });
    expect(session.focused).toBe(1);
    expect(said.at(-1)).toBe("documnet: document");
    expect(await store.next()).toBe(true);
    expect(session.revealed.at(-1)).toEqual({ from: 6, to: 11 });
    expect(said.at(-1)).toBe("wrold: world");
  });

  it("a caret on no mark: from the end of the selection, else from the start of its block", async () => {
    await startWith("Hello wrold and documnet");
    session.caret = 13;
    session.scopes.paragraph = { from: 0, to: 24 };
    await store.next();
    expect(session.revealed.at(-1)).toEqual({ from: 6, to: 11 });
    session.caret = 13;
    session.scopes.selection = { from: 12, to: 14 };
    await store.next();
    expect(session.revealed.at(-1)).toEqual({ from: 16, to: 24 });
  });

  it("nothing to step to in View or without marks", async () => {
    await startWith("Hello world");
    expect(store.canStep).toBe(false);
    expect(await store.next()).toBe(false);
    session.text = "Hello wrold";
    await settle();
    session.mode = "view";
    await settle();
    expect(store.canStep).toBe(false);
  });
});

describe("the word being typed", () => {
  it("is marked once the caret leaves it, with no other change", async () => {
    await startWith("Hello");
    session.text = "Hello wrold";
    session.caret = 11;
    await settle();
    expect(session.marked()).toEqual([]);
    // the caret moves inside the word: still being typed
    session.caret = 9;
    document.dispatchEvent(new Event("selectionchange"));
    await vi.advanceTimersByTimeAsync(CARET_MS);
    expect(session.marked()).toEqual([]);
    session.caret = 2;
    document.dispatchEvent(new Event("selectionchange"));
    await vi.advanceTimersByTimeAsync(CARET_MS);
    expect(session.marked()).toEqual(["wrold"]);
  });
});

describe("the status bar", () => {
  it("the language and where it comes from: the default, declared, detected", async () => {
    await startWith("Hello wrold");
    expect(store.status).toEqual({
      kind: "language",
      lang: "en",
      source: "default",
      missing: false,
    });
    expect(await store.declare("pt-BR")).toBe(true);
    expect(store.status).toEqual({
      kind: "language",
      lang: "pt-BR",
      source: "declared",
      missing: true,
    });
    expect(await store.declare(null)).toBe(true);
    session.text = "The guide is on the folder and the file is with the note of the project";
    session.path = "docs/b.md";
    await settle();
    expect(store.status).toMatchObject({ lang: "en", source: "detected" });
  });

  it("a writing system no dictionary covers; nothing in View or without a document", async () => {
    await startWith("Привет мир это текст на русском языке для проверки");
    expect(store.status).toEqual({ kind: "unsupported" });
    session.mode = "view";
    expect(store.status).toEqual({ kind: "none" });
    session.mode = "edit";
    session.path = null;
    expect(store.status).toEqual({ kind: "none" });
  });
});

describe("the language picker", () => {
  it("Detect automatically and the dictionaries, each with where it stands; a ready one is declared", async () => {
    mock.probe.spelling.setState("fr", { kind: "downloading", done: 50, total: 100 });
    await startWith("Hello wrold");
    const entries = languageEntries(store, { x: 0, y: 0 });
    expect(entries[0]).toMatchObject({
      label: "Detect automatically",
      detail: "Now: English",
    });
    expect(entryOf(entries, "German")).toMatchObject({ detail: "Download, 1 MB" });
    expect(entryOf(entries, "French")).toMatchObject({ detail: "Downloading, 50 %" });
    run(entries, "English");
    expect(mock.probe.spelling.calls).toContain("declare docs/a.md en");
  });

  it("a dictionary is fetched only after the person confirms the question", async () => {
    await startWith("Hello wrold");
    run(languageEntries(store, { x: 0, y: 0 }), "German");
    await tick();
    expect(store.asking).toEqual({ lang: "de", declare: true });
    expect(mock.probe.spelling.calls.filter((c) => c.startsWith("download"))).toEqual([]);
    store.closeAsk();
    expect(mock.probe.spelling.calls.filter((c) => c.startsWith("download"))).toEqual([]);
    store.askDownload("de", true);
    await store.confirmDownload();
    expect(mock.probe.spelling.calls).toContain("download de");
    expect(store.document?.language).toMatchObject({ langs: ["de"], source: "declared" });
    expect(store.languageOf("de")?.state.kind).toBe("downloading");
    await settle();
    expect(store.languageOf("de")?.state.kind).toBe("ready");
    expect(store.needsDownload).toEqual([]);
  });

  it("the document's language without its dictionary: the download offered first", async () => {
    mock.probe.spelling.declared.set("docs/a.md", "pt-BR");
    await startWith("Olá mundo");
    const entries = languageEntries(store, { x: 0, y: 0 });
    expect(entries[0]).toMatchObject({ label: "Download the Portuguese (Brazil) dictionary…" });
    run(entries, "Download the Portuguese (Brazil) dictionary…");
    await tick();
    expect(store.asking).toEqual({ lang: "pt-BR", declare: false });
  });

  it("a failed download this window started: a notice with Try again", async () => {
    await startWith("Hello wrold");
    mock.probe.spelling.failNext = { kind: "offline" };
    expect(await store.download("de")).toBe(true);
    await settle();
    expect(notices).toEqual([
      expect.objectContaining({
        key: "spelling.download.de",
        text: "The German dictionary could not be downloaded. No connection to the download site.",
      }),
    ]);
    await notices[0]?.actions[0]?.run();
    expect(mock.probe.spelling.calls.filter((c) => c === "download de")).toHaveLength(2);
  });

  it("the default for new documents, in place of the list", async () => {
    await startWith("Hello wrold");
    store.openPicker(5, 6);
    await settle();
    const opener = entryOf(
      languageEntries(store, { x: 5, y: 6 }),
      "Default for new documents: English",
    );
    opener?.run(false);
    expect(store.picker).toEqual({ x: 5, y: 6, level: "default", back: false });
    run(defaultEntries(store), "French");
    await settle();
    expect(store.defaultLanguage).toEqual({ lang: "fr", chosen: true });
    // the document was checked in the default: it follows
    expect(store.document?.language.langs).toEqual(["fr"]);
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

  it("off: the keyboard's menu and the caret are the webview's again; no picker, no question", async () => {
    await startWith("Hello wrold");
    store.openPicker(0, 0);
    store.askDownload("de", true);
    stop();
    expect(store.picker).toBeNull();
    expect(store.asking).toBeNull();
    session.caret = 8;
    expect(contextMenu(editorElement(), 0).defaultPrevented).toBe(false);
    expect(store.menuAt).toBeNull();
  });
});

describe("what the person sees", () => {
  let component: Record<string, unknown> | null = null;
  let target: HTMLElement;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    component = null;
  });

  function showStatus(): HTMLButtonElement | null {
    component = mount(SpellingStatus, { target, props: { store } });
    flushSync();
    return target.querySelector<HTMLButtonElement>("[data-spelling-status]");
  }

  const menuItems = (): HTMLElement[] => [
    ...document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]'),
  ];
  const itemLabel = (item: HTMLElement): string =>
    (item.querySelector(".label") ?? item).textContent?.trim() ?? "";
  const itemNamed = (label: string): HTMLElement | undefined =>
    menuItems().find((item) => itemLabel(item) === label);

  it("the status item: the short name, where it comes from in its tooltip", async () => {
    await startWith("Hello wrold");
    const button = showStatus();
    expect(button?.textContent?.trim()).toBe("English");
    expect(button?.title).toBe("Spelling: English, the default language");
    await store.declare("pt-BR");
    flushSync();
    expect(button?.textContent?.trim()).toBe("Portuguese (BR)");
    expect(button?.classList.contains("is-missing")).toBe(true);
    expect(button?.title).toBe(
      "Spelling: Portuguese (Brazil), chosen for this document. Its dictionary isn't downloaded yet.",
    );
  });

  it("the status item: Not available for a writing system no dictionary covers; none in View", async () => {
    await startWith("Привет мир это текст на русском языке для проверки");
    const button = showStatus();
    expect(button?.textContent?.trim()).toBe("Not available");
    session.mode = "view";
    flushSync();
    expect(target.querySelector("[data-spelling-status]")).toBeNull();
  });

  it("a click opens the picker; a dictionary not here is asked about, fetched only on Download", async () => {
    await startWith("Hello wrold");
    showStatus()?.click();
    flushSync();
    await settle();
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "Spelling language",
    );
    itemNamed("German")?.click();
    await tick();
    flushSync();
    const dialog = document.querySelector('[role="dialog"]');
    expect(dialog?.querySelector("h2")?.textContent?.trim()).toBe(
      "Download the German dictionary (1 MB, license GPL-2.0 OR GPL-3.0)?",
    );
    expect(mock.probe.spelling.calls.filter((c) => c.startsWith("download"))).toEqual([]);
    dialog?.querySelector<HTMLButtonElement>("[data-initial]")?.click();
    await settle();
    expect(mock.probe.spelling.calls).toContain("download de");
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it("a right-click on a mark: its menu with the corrections; a pick replaces the word, the keyboard back in the editor", async () => {
    await startWith("Hello wrold");
    showStatus();
    const editor = editorElement();
    editor.tabIndex = 0;
    editor.focus();
    session.last()?.options.menu?.(0, 10, 20);
    await settle();
    flushSync();
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "Spelling of “wrold”",
    );
    expect(menuItems().map(itemLabel).slice(-3)).toEqual([
      "Add to dictionary",
      "Ignore in this document",
      "Language: English…",
    ]);
    itemNamed("world")?.click();
    flushSync();
    expect(session.applied).toEqual([[{ from: 6, to: 11, insert: "world", expect: "wrold" }]]);
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(editor);
  });

  it("the settings: the person's words with a remove button each", async () => {
    await startWith("Hello wrold");
    await store.addToDictionary("wrold");
    component = mount(SpellingSettings, { target, props: { store } });
    flushSync();
    await settle();
    flushSync();
    const remove = target.querySelector<HTMLButtonElement>('[aria-label="Remove wrold"]');
    expect(remove).not.toBeNull();
    remove?.click();
    await settle();
    flushSync();
    expect(store.personal).toEqual([]);
    expect(target.querySelector('[aria-label="Remove wrold"]')).toBeNull();
    expect(target.textContent).toContain("English");
  });
});

/** The labels of a menu's items. */
function labels(entries: readonly MenuEntry[]): string[] {
  return entries.flatMap((entry) => (entry.kind === "item" ? [entry.label] : []));
}

function entryOf(
  entries: readonly MenuEntry[],
  label: string,
): Extract<MenuEntry, { kind: "item" }> | undefined {
  return entries.find(
    (entry): entry is Extract<MenuEntry, { kind: "item" }> =>
      entry.kind === "item" && entry.label === label,
  );
}

/** Runs the menu item `label` as a pick would. */
function run(entries: readonly MenuEntry[], label: string): void {
  const entry = entryOf(entries, label);
  if (entry === undefined) throw new Error(`no entry ${label}`);
  entry.run(false);
}

/** An editor's text root in the page. */
function editorElement(): HTMLElement {
  const editor = document.createElement("div");
  editor.className = "cm-content";
  document.body.append(editor);
  return editor;
}

/** The browser's context menu event on `target` (`button` 2: the pointer; 0: the keyboard). */
function contextMenu(target: HTMLElement, button: number): MouseEvent {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
    button,
    clientX: 30,
    clientY: 40,
  });
  target.dispatchEvent(event);
  return event;
}

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
