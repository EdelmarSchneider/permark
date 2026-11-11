// @vitest-environment jsdom
// The piece review bar as a frame: its words and its excerpt come from the plugin (here: words of other features and
// an excerpt of two stacked lines), nothing of one plugin is built in. The bar through a run, its keys and its focus
// are driven where a plugin configures it: `plugins/correction/bar.test.ts`.
import { createRawSnippet, flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiError } from "../../../api";
import type { ReviewLabels } from "./ReviewBar.svelte";
import type { Found, ReviewState } from "./review.svelte";

vi.mock("../../../api", async () => {
  const { createMockApi } = await import("../../../api/mock");
  return { api: createMockApi() };
});

const { t, tn } = await import("../../../core/i18n");
const { PieceReview } = await import("./review.svelte");
const { default: ReviewBar } = await import("./ReviewBar.svelte");
type Review = Extract<ReviewState, { kind: "review" }>;

const review = new PieceReview({
  plugin: "correction",
  prepare: () => [],
  judge: () => ({ kind: "ok", suggestions: [] }),
  runArgs: () => ({}),
});

/** Words of other features standing in for a plugin's. */
const LABELS: ReviewLabels = {
  bar: "review.bar",
  nothing: "review.allDone",
  loading: "review.review",
  checking: "review.kind.modified",
  clean: "review.keptAll",
  found: "review.reviewing",
  stopped: "review.offer",
  failed: "common.details",
  noModel: "localAi.err.notInstalled",
  position: "review.position",
  prev: "review.prev",
  next: "review.next",
  show: "review.undo",
  discarded: "search.files",
  outdated: "finder.results",
  retry: "common.dismiss",
  openSettings: "common.close",
  accept: "review.keepAll",
  reject: "review.revertAll",
  acceptAll: "review.keepAllHint",
  rejectAll: "review.revertAllHint",
  done: "review.done",
  spoken: (s, at, n) => `${at + 1}/${n}: ${s.original} > ${s.insert}`,
};

/** A plugin's excerpt: what is there over what would go there. */
const excerpt = createRawSnippet<[Found]>((s) => ({
  render: () =>
    `<span class="stack"><span>${s().original}</span><br><span>${s().insert}</span></span>`,
}));

const S1: Found = {
  key: 1,
  from: 2,
  to: 5,
  original: "has",
  insert: "have",
  before: "I ",
  after: " a cat.",
};
const S2: Found = { ...S1, key: 2, from: 40, to: 43, original: "teh", insert: "the" };

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

function state(patch: Partial<Review>): Review {
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

function show(next: ReviewState): void {
  review.state = next;
  flushSync();
}

const said = (): string => (target.textContent ?? "").replace(/\s+/g, " ").trim();
const button = (action: string): HTMLButtonElement | null =>
  target.querySelector<HTMLButtonElement>(`[data-action="${action}"]`);
const group = (): HTMLElement | null => target.querySelector<HTMLElement>('[role="group"]');

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
  component = mount(ReviewBar, { target, props: { review, labels: LABELS, excerpt } });
  flushSync();
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  review.state = { kind: "closed" };
  document.body.replaceChildren();
});

describe("the piece review bar", () => {
  it("says the plugin's words: the bar's name, where the run stands, the notes, the actions", () => {
    show({ kind: "nothing", locked: 0 });
    expect(group()?.getAttribute("aria-label")).toBe(t("review.bar"));
    expect(said()).toContain(t("review.allDone"));
    show(state({ suggestions: [S1, S2], discarded: 1, outdated: 2 }));
    expect(target.querySelector(".status")?.textContent).toBe(tn("review.reviewing", 2));
    expect(said()).toContain(tn("search.files", 1));
    expect(said()).toContain(tn("finder.results", 2));
    const actions = ["accept", "reject", "acceptAll", "rejectAll", "done"];
    expect(actions.map((action) => button(action)?.textContent?.trim())).toEqual([
      t("review.keepAll"),
      t("review.revertAll"),
      t("review.keepAllHint"),
      t("review.revertAllHint"),
      t("review.done"),
    ]);
    // found in the page by its plugin's name
    expect(group()?.hasAttribute("data-correction")).toBe(true);
  });

  it("draws the plugin's excerpt in the button that shows the suggestion, says its words for it", () => {
    show(state({ suggestions: [S1] }));
    const shown = button("show");
    expect(shown?.getAttribute("aria-label")).toBe(t("review.undo"));
    const lines = shown?.querySelectorAll('[aria-hidden="true"] .stack > span') ?? [];
    expect([...lines].map((line) => line.textContent)).toEqual(["has", "have"]);
    // no marks of its own around what the plugin drew
    expect(target.querySelector("del, ins")).toBeNull();
    expect(target.querySelector("[aria-live]")?.textContent).toContain("1/1: has > have");
  });

  it("answers that said the same as their piece: a note of its own, only from a plugin with words for it", () => {
    const notes = (): string[] =>
      [...target.querySelectorAll("p.quiet")].map((note) => note.textContent ?? "");
    show(state({ same: 2, discarded: 1 }));
    expect(notes()).toEqual([tn("search.files", 1)]);
    if (component) void unmount(component);
    const labels: ReviewLabels = { ...LABELS, same: "wordCount.words" };
    component = mount(ReviewBar, { target, props: { review, labels, excerpt } });
    flushSync();
    expect(notes()).toEqual([tn("search.files", 1), tn("wordCount.words", 2)]);
    show(state({ same: 0, discarded: 1 }));
    expect(notes()).toEqual([tn("search.files", 1)]);
    // words that need more than the count (a language's name): the plugin's own
    if (component) void unmount(component);
    const own: ReviewLabels = { ...LABELS, same: (n) => `${n} already in German` };
    component = mount(ReviewBar, { target, props: { review, labels: own, excerpt } });
    flushSync();
    show(state({ same: 3 }));
    expect(notes()).toEqual(["3 already in German"]);
  });

  it("parts the editor on screen cannot change: a note in the frame's own words; with nothing else to say, what the bar says", () => {
    const notes = (): string[] =>
      [...target.querySelectorAll("p.quiet")].map((note) => note.textContent ?? "");
    show(state({ suggestions: [S1], total: 1, locked: 2, discarded: 1 }));
    expect(notes()).toEqual(["2 parts can only be changed in Source.", tn("search.files", 1)]);
    show(state({ suggestions: [S1], total: 1 }));
    expect(notes()).toEqual([]);
    // nothing was sent because of them: said in the place of the plugin's "nothing here"
    show({ kind: "nothing", locked: 1 });
    expect(target.querySelector('[role="status"]')?.textContent).toBe(
      "1 part can only be changed in Source.",
    );
    expect(said()).not.toContain(t("review.allDone"));
    expect(said()).toContain(t("review.done"));
  });

  it("the run's end with none left: the plugin's words when none was found, the frame's own when all are handled", () => {
    const status = (): string => target.querySelector(".status")?.textContent ?? "";
    show(state({}));
    expect(status()).toBe(t("review.keptAll"));
    show(state({ same: 2 }));
    expect(status()).toBe(t("review.keptAll"));
    // found, then accepted | rejected | left behind by the text
    show(state({ total: 3, outdated: 1 }));
    expect(status()).toBe("All reviewed.");
    expect(target.querySelector("[aria-live]")?.textContent).toBe("All reviewed.");
    show(state({ total: 3, suggestions: [S1] }));
    expect(status()).toBe(tn("review.reviewing", 1));
    // a stopped run says what it says
    show(state({ status: { kind: "cancelled" }, total: 3 }));
    expect(status()).toBe(tn("review.offer", 0));
  });

  it("an excerpt that stands on its own: drawn as the plugin draws it, no button of the frame around it", () => {
    if (component) void unmount(component);
    component = mount(ReviewBar, {
      target,
      props: { review, labels: LABELS, excerpt, excerptAs: "own" },
    });
    flushSync();
    show(state({ suggestions: [S1, S2] }));
    expect(button("show")).toBeNull();
    const stack = target.querySelector(".stack");
    expect(stack?.parentElement).toBe(group());
    // not cut and not hidden from assistive tech by the frame: sizes and names are the plugin's
    expect(stack?.closest("button, [aria-hidden]")).toBeNull();
    expect([...(stack?.querySelectorAll(":scope > span") ?? [])].map((l) => l.textContent)).toEqual(
      ["has", "have"],
    );
    // the rest of the bar as ever
    expect(button("accept")?.disabled).toBe(false);
    expect(target.querySelector("[aria-live]")?.textContent).toContain("1/2: has > have");
  });

  it("a failed run: the plugin's words for no model on its job, the engine's own for the rest", () => {
    const error = (subject: string, detail: string): ApiError => ({
      kind: "backend",
      command: "plugin_call",
      error: { kind: "unavailable", subject, detail },
    });
    show(state({ status: { kind: "failed", error: error("model", "noModel") } }));
    expect(target.querySelector('[role="alert"]')?.textContent).toContain(
      t("localAi.err.notInstalled"),
    );
    show(state({ status: { kind: "failed", error: error("engine", "notAvailable") } }));
    expect(said()).toContain(t("localAi.test.failed.notAvailable"));
    expect(said()).not.toContain(t("localAi.err.notInstalled"));
  });
});
