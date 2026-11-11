// @vitest-environment jsdom
// The translation bar: the part as it is over its translation (each named, the translation by the language its run
// was sent; cut to three lines until asked for more), the words through a run, the notes at its end, the failure and
// the buttons on the store. The bar's own rules (focus, live region, keys) are driven where correction configures
// it: `plugins/correction/bar.test.ts`.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiError } from "../../api";
import type { Found } from "../local-ai/pieces/review.svelte";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { language } = await import("../../core/i18n");
const { ui } = await import("../../core/shell/ui.svelte");
const { translation } = await import("./store.svelte");
const { default: TranslationBar } = await import("./TranslationBar.svelte");
type Review = Extract<typeof translation.state, { kind: "review" }>;

const SHORT = "The weather is nice today.";
const LONG = "It rained for a week, and then the river rose over the road. ".repeat(6).trim();
const S1: Found = {
  key: 1,
  from: 0,
  to: SHORT.length,
  original: SHORT,
  insert: "Das Wetter ist heute schön.",
  before: "",
  after: "",
};
const S2: Found = {
  ...S1,
  key: 2,
  from: 40,
  to: 40 + LONG.length,
  original: LONG,
  insert: "Kurz.",
};

const GERMAN = { tag: "de", name: "German" };
const ENGLISH = { tag: "en-US", name: "American English" };

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

function review(patch: Partial<Review>): Review {
  return {
    kind: "review",
    status: { kind: "finished" },
    suggestions: [],
    at: 0,
    discarded: 0,
    same: 0,
    outdated: 0,
    locked: 0,
    total: 0,
    ...patch,
  };
}

function show(state: typeof translation.state): void {
  translation.state = state;
  flushSync();
}

const said = (el: Element | null = target): string =>
  (el?.textContent ?? "").replace(/\s+/g, " ").trim();
const button = (action: string, within: ParentNode = target): HTMLButtonElement | null =>
  within.querySelector<HTMLButtonElement>(`[data-action="${action}"]`);
const group = (): HTMLElement | null => target.querySelector<HTMLElement>('[role="group"]');
const block = (kind: "original" | "translation"): HTMLElement => {
  const found = target.querySelector<HTMLElement>(`[data-block="${kind}"]`);
  if (found === null) throw new Error(`no ${kind} block`);
  return found;
};
const label = (kind: "original" | "translation"): string =>
  said(block(kind).querySelector(".label"));
const text = (kind: "original" | "translation"): HTMLElement => {
  const found = block(kind).querySelector<HTMLElement>("[data-text]");
  if (found === null) throw new Error(`no ${kind} text`);
  return found;
};
const notes = (): string[] => [...target.querySelectorAll("p.quiet")].map((note) => said(note));

/** jsdom lays nothing out: a text of more than 100 characters stands for one longer than three lines. */
function layout(): void {
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get: () => 60,
  });
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
    configurable: true,
    get(this: HTMLElement) {
      return (this.textContent ?? "").length > 100 ? 200 : 60;
    },
  });
}

beforeEach(() => {
  layout();
  translation.languages.sent = { target: GERMAN, source: null };
  target = document.createElement("div");
  document.body.append(target);
  component = mount(TranslationBar, { target });
  flushSync();
});

afterEach(async () => {
  if (component) void unmount(component);
  component = null;
  translation.state = { kind: "closed" };
  translation.languages.reset();
  vi.restoreAllMocks();
  Reflect.deleteProperty(HTMLElement.prototype, "clientHeight");
  Reflect.deleteProperty(HTMLElement.prototype, "scrollHeight");
  document.body.replaceChildren();
  await language.choose("en", []);
});

describe("the translation bar", () => {
  it("draws nothing before a run", () => {
    expect(target.innerHTML.replace(/<!---->/g, "").trim()).toBe("");
  });

  it("nothing to translate in the scope: says so, Done", () => {
    show({ kind: "nothing", locked: 0 });
    expect(said()).toContain("There's no text to translate here.");
    expect(group()?.getAttribute("aria-label")).toBe("Translations to review");
    expect(group()?.hasAttribute("data-translation")).toBe(true);
  });

  it("the part as it is over its translation, each under its name", () => {
    show(review({ suggestions: [S1, S2] }));
    expect(
      [...target.querySelectorAll("[data-block]")].map((el) => el.getAttribute("data-block")),
    ).toEqual(["original", "translation"]);
    expect(label("original")).toBe("Original");
    expect(said(text("original"))).toBe(SHORT);
    expect(label("translation")).toBe("Translation · German");
    expect(said(text("translation"))).toBe("Das Wetter ist heute schön.");
    // plain text: no marks of removed | added words
    expect(target.querySelector("del, ins")).toBeNull();
    // read by assistive tech in its own language, laid out by its own direction
    expect(text("translation").getAttribute("lang")).toBe("de");
    expect(text("original").hasAttribute("lang")).toBe(false);
    expect(text("original").getAttribute("dir")).toBe("auto");
    expect(text("translation").getAttribute("dir")).toBe("auto");
    expect(text("original").closest("[aria-hidden]")).toBeNull();
  });

  it("names the language of the original only when the person named it", () => {
    translation.languages.sent = { target: GERMAN, source: ENGLISH };
    show(review({ suggestions: [S1] }));
    expect(label("original")).toBe("Original · English (United States)");
    expect(text("original").getAttribute("lang")).toBe("en-US");
  });

  it("names the languages its run was sent, in the app's language, whatever is chosen since", async () => {
    await language.choose("pt-BR", []);
    translation.languages.target = "fr";
    show(review({ suggestions: [S1], same: 2 }));
    expect(label("original")).toBe("Original");
    expect(label("translation")).toBe("Tradução · Alemão");
    expect(text("translation").getAttribute("lang")).toBe("de");
    // inside a sentence the name is written as the language writes it there
    expect(notes()).toEqual(["2 partes já estavam em alemão."]);
  });

  it("a text longer than three lines is cut, with Show more | Show less; the next one starts cut again", () => {
    show(review({ suggestions: [S1, S2] }));
    // both fit
    expect(button("more")).toBeNull();
    show(review({ suggestions: [S1, S2], at: 1 }));
    const more = button("more", block("original"));
    expect(said(more)).toBe("Show more");
    expect(more?.getAttribute("aria-expanded")).toBe("false");
    expect(text("original").classList.contains("cut")).toBe(true);
    // the short translation needs none
    expect(button("more", block("translation"))).toBeNull();
    more?.click();
    flushSync();
    expect(said(button("more", block("original")))).toBe("Show less");
    expect(button("more", block("original"))?.getAttribute("aria-expanded")).toBe("true");
    expect(text("original").classList.contains("cut")).toBe(false);
    button("more", block("original"))?.click();
    flushSync();
    expect(button("more", block("original"))?.getAttribute("aria-expanded")).toBe("false");
    // shown in full, then on to another suggestion and back: cut again
    button("more", block("original"))?.click();
    show(review({ suggestions: [S1, S2], at: 0 }));
    show(review({ suggestions: [S1, S2], at: 1 }));
    expect(button("more", block("original"))?.getAttribute("aria-expanded")).toBe("false");
  });

  it("each text and each Show more is named with the block it belongs to: four buttons, four names", () => {
    /** What assistive tech calls `el`: the text of the elements its `aria-labelledby` lists, in that order. */
    const named = (el: Element | null): string =>
      (el?.getAttribute("aria-labelledby") ?? "")
        .split(" ")
        .map((id) => said(document.getElementById(id)))
        .join(" ");
    const longer = "Es regnete eine Woche lang, und dann stieg der Fluss über die Straße. ".repeat(
      6,
    );
    translation.languages.sent = { target: GERMAN, source: ENGLISH };
    show(review({ suggestions: [{ ...S2, insert: longer.trim() }] }));
    const names = [
      text("original"),
      button("more", block("original")),
      text("translation"),
      button("more", block("translation")),
    ].map(named);
    expect(names).toEqual([
      `${label("original")} ${LONG}`,
      `Show more ${label("original")}`,
      `Translation · German ${longer.trim()}`,
      "Show more Translation · German",
    ]);
    expect(label("original")).toMatch(/^Original · English/);
    expect(new Set(names).size).toBe(4);
    // what a click on a text does is still said of it
    expect(text("translation").getAttribute("title")).toBe("Show in the text");
    expect(text("translation").hasAttribute("aria-label")).toBe(false);
    // shown in full: the button says so under the same block's name
    button("more", block("translation"))?.click();
    flushSync();
    expect(named(button("more", block("translation")))).toBe("Show less Translation · German");
  });

  it("a click on either text shows the suggestion in the document", () => {
    const shown = vi.spyOn(translation, "show").mockImplementation(() => {});
    show(review({ suggestions: [S1] }));
    expect(text("original").tagName).toBe("BUTTON");
    expect(text("original").getAttribute("title")).toBe("Show in the text");
    text("original").click();
    text("translation").click();
    expect(shown).toHaveBeenCalledTimes(2);
  });

  it("through a run: the model, part i of n, the count; all-at-once waits for the end", () => {
    show(review({ status: { kind: "loading", progress: 0.5 } }));
    expect(said(target.querySelector('[role="status"]'))).toContain("Getting the model ready…");
    expect(target.querySelector("progress")?.value).toBe(0.5);
    expect(button("cancel")).not.toBeNull();
    show(review({ status: { kind: "checking", part: 2, parts: 3 }, suggestions: [S1, S2] }));
    expect(said()).toContain("Translating part 2 of 3…");
    expect(said()).toContain("1 / 2");
    expect(button("accept")?.disabled).toBe(false);
    expect(button("acceptAll")?.disabled).toBe(true);
    expect(button("rejectAll")?.disabled).toBe(true);
    // one live region: the current translation, not every part
    const live = target.querySelectorAll("[aria-live]");
    expect(live).toHaveLength(1);
    expect(said(live[0] ?? null)).toBe("Translation 1 of 2: Das Wetter ist heute schön.");
  });

  it("the end: the count, nothing to translate, stopped", () => {
    show(review({ suggestions: [S1, S2] }));
    expect(said(target.querySelector(".status"))).toBe("2 translations");
    show(review({ suggestions: [S1] }));
    expect(said(target.querySelector(".status"))).toBe("1 translation");
    show(review({ same: 3 }));
    expect(said(target.querySelector(".status"))).toBe("Nothing to translate here.");
    // translations were found and each one is handled since
    show(review({ total: 2, outdated: 1 }));
    expect(said(target.querySelector(".status"))).toBe("All reviewed.");
    show(review({ status: { kind: "cancelled" }, suggestions: [S1] }));
    expect(said(target.querySelector(".status"))).toBe("Stopped. 1 translation ready.");
    show(review({ status: { kind: "cancelled" }, suggestions: [S1, S2] }));
    expect(said(target.querySelector(".status"))).toBe("Stopped. 2 translations ready.");
  });

  it("the notes: parts that could not be translated, parts already in the language, translations the text left behind", () => {
    show(review({ suggestions: [S1], discarded: 1, same: 1, outdated: 1 }));
    expect(notes()).toEqual([
      "1 part couldn't be translated and was left as it is.",
      "1 part was already in German.",
      "1 translation no longer applies because the text changed.",
    ]);
    show(review({ suggestions: [S1], discarded: 2, same: 3, outdated: 4 }));
    expect(notes()).toEqual([
      "2 parts couldn't be translated and were left as they are.",
      "3 parts were already in German.",
      "4 translations no longer apply because the text changed.",
    ]);
    // parts the editor on screen keeps as written were not sent
    show(review({ suggestions: [S1], total: 1, locked: 2, outdated: 1 }));
    expect(notes()).toEqual([
      "2 parts can only be changed in Source.",
      "1 translation no longer applies because the text changed.",
    ]);
    // never the words of the machinery
    expect(said()).not.toMatch(/piece|segment|output/i);
  });

  it("the buttons act on the review", () => {
    const calls: string[] = [];
    for (const name of ["accept", "reject", "acceptAll", "rejectAll", "done", "cancel"] as const)
      vi.spyOn(translation, name).mockImplementation(() => void calls.push(name));
    const go = vi.spyOn(translation, "go").mockImplementation(() => {});
    show(review({ suggestions: [S1, S2] }));
    const names = ["accept", "reject", "acceptAll", "rejectAll", "done"];
    expect(names.map((name) => said(button(name)))).toEqual([
      "Accept",
      "Reject",
      "Accept all",
      "Reject all",
      "Done",
    ]);
    for (const name of names) button(name)?.click();
    expect(calls).toEqual(names);
    const [prev, next] = [...target.querySelectorAll<HTMLButtonElement>("button.icon")];
    expect([prev, next].map((el) => el?.getAttribute("aria-label"))).toEqual([
      "Previous translation",
      "Next translation",
    ]);
    prev?.click();
    next?.click();
    expect(go.mock.calls).toEqual([[-1], [1]]);
    show(review({ status: { kind: "checking", part: 1, parts: 2 }, suggestions: [S1] }));
    expect(said(button("cancel"))).toBe("Cancel");
    button("cancel")?.click();
    expect(calls.at(-1)).toBe("cancel");
  });

  it("the bar's keys act from the texts too: A accepts, R rejects", () => {
    const calls: string[] = [];
    for (const name of ["accept", "reject"] as const)
      vi.spyOn(translation, name).mockImplementation(() => void calls.push(name));
    show(review({ suggestions: [S1] }));
    for (const key of ["a", "r"])
      text("translation").dispatchEvent(
        new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
      );
    expect(calls).toEqual(["accept", "reject"]);
  });

  it("failed: its words, Try again, the way to the Local AI page", () => {
    const error = (subject: string, detail: string): ApiError => ({
      kind: "backend",
      command: "plugin_call",
      error: { kind: "unavailable", subject, detail },
    });
    const retry = vi.spyOn(translation, "retry").mockResolvedValue();
    const open = vi.spyOn(ui, "openSettingsPage").mockImplementation(() => {});
    show(review({ status: { kind: "failed", error: error("model", "noModel") } }));
    expect(said(target.querySelector(".status"))).toBe("The text couldn't be translated.");
    const alert = (): Element | null => target.querySelector('[role="alert"]');
    expect(said(alert())).toContain(
      "The model chosen for translation isn't available. Choose one on the Local AI page.",
    );
    const [again, settings] = [...(alert()?.querySelectorAll("button") ?? [])];
    expect([again, settings].map((el) => said(el ?? null))).toEqual([
      "Try again",
      "Open Local AI settings",
    ]);
    again?.click();
    settings?.click();
    expect(retry).toHaveBeenCalledOnce();
    expect(open).toHaveBeenCalledWith("local-ai.page", "content");
    // the engine's own words for the rest
    show(review({ status: { kind: "failed", error: error("engine", "outOfMemory") } }));
    expect(said(alert())).toContain("Not enough free memory right now.");
    show(review({ status: { kind: "failed", error: error("engine", "engineTooOld") } }));
    expect(said(alert())).toContain("This model needs a newer version of the AI engine.");
  });
});
