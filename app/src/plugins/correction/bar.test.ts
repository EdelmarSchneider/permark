// @vitest-environment jsdom
// The correction bar per state: nothing before a run, "nothing to check", getting the AI engine (first run: sizes,
// Cancel), getting the model ready (progress, Cancel),
// checking (part i of n, the suggestion with its removed | added words and what a screen reader hears), the end
// (count, no mistakes, notes), stopped, failed (its words, Try again, Local AI settings); the keys inside the bar.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiError } from "../../api";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { correction } = await import("./store.svelte");
const { ui } = await import("../../core/shell/ui.svelte");
const { default: CorrectionBar } = await import("./CorrectionBar.svelte");
type Review = Extract<typeof correction.state, { kind: "review" }>;

const S1 = {
  key: 1,
  from: 2,
  to: 5,
  original: "has",
  insert: "have",
  before: "I ",
  after: " a cat.\nNext line…",
};
const S2 = { ...S1, key: 2, from: 40, to: 43, original: "teh", insert: "the", before: "x " };

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

function review(patch: Partial<Review>): Review {
  return {
    kind: "review",
    status: { kind: "finished" },
    suggestions: [],
    at: 0,
    discarded: 0,
    outdated: 0,
    ...patch,
  };
}

function show(state: typeof correction.state): void {
  correction.state = state;
  flushSync();
}

const said = (): string => (target.textContent ?? "").replace(/\s+/g, " ").trim();
const button = (action: string): HTMLButtonElement | null =>
  target.querySelector<HTMLButtonElement>(`[data-action="${action}"]`);
const group = (): HTMLElement | null => target.querySelector<HTMLElement>('[role="group"]');

function key(init: KeyboardEventInit, on: Element | null = button("done")): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  on?.dispatchEvent(event);
  return event;
}

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
  component = mount(CorrectionBar, { target });
  flushSync();
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  correction.state = { kind: "closed" };
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe("the correction bar", () => {
  it("draws nothing before a run", () => {
    expect(target.innerHTML.replace(/<!---->/g, "").trim()).toBe("");
  });

  it("nothing to check: says so, Done", () => {
    show({ kind: "nothing" });
    expect(said()).toContain("There's no text to check here.");
    expect(group()?.getAttribute("aria-label")).toBe("Spelling and grammar suggestions");
  });

  it("getting the model ready: progress and Cancel, nothing to accept yet", () => {
    show(review({ status: { kind: "loading", progress: 0.5 } }));
    const status = target.querySelector('[role="status"]');
    expect(status?.getAttribute("aria-live")).toBe("polite");
    expect(status?.textContent).toContain("Getting the model ready…");
    expect(target.querySelector("progress")?.value).toBe(0.5);
    expect(button("cancel")).not.toBeNull();
    expect(button("accept")?.disabled).toBe(true);
    expect(button("acceptAll")?.disabled).toBe(true);
  });

  it("getting the AI engine (first run only): its size progress and Cancel", () => {
    show(review({ status: { kind: "engine", receivedBytes: 12e6, totalBytes: 48e6 } }));
    expect(said()).toContain("Getting the AI engine (first time only)");
    expect(said()).toContain("12 MB of 48 MB");
    const progress = target.querySelector("progress");
    expect(progress?.max).toBe(48e6);
    expect(progress?.value).toBe(12e6);
    expect(target.querySelector('[role="status"]')?.textContent).toContain(
      "Getting the AI engine (first time only)",
    );
    expect(button("cancel")).not.toBeNull();
    expect(button("accept")?.disabled).toBe(true);
  });

  it("checking: part i of n, the suggestion in its text, all-at-once waits for the end", () => {
    show(review({ status: { kind: "checking", part: 2, parts: 3 }, suggestions: [S1, S2], at: 0 }));
    expect(said()).toContain("Checking part 2 of 3…");
    expect(said()).toContain("1 / 2");
    expect(target.querySelector("del")?.textContent).toBe("has");
    expect(target.querySelector("ins")?.textContent).toBe("have");
    // the excerpt as the store cut it (its block, word edges)
    expect(target.querySelector("del")?.closest('[aria-hidden="true"]')?.textContent).toBe(
      "I hashave a cat.\nNext line…",
    );
    // one live region: the current step, not every part checked
    const live = target.querySelectorAll("[aria-live]");
    expect(live).toHaveLength(1);
    expect(live[0]?.textContent?.trim()).toBe("Suggestion 1 of 2: replace “has” with “have”");
    expect(button("accept")?.disabled).toBe(false);
    expect(button("acceptAll")?.disabled).toBe(true);
    expect(button("rejectAll")?.disabled).toBe(true);
  });

  it("the end: the count, then the notes", () => {
    show(review({ suggestions: [S1, S2], discarded: 1, outdated: 2 }));
    expect(said()).toContain("2 suggestions");
    expect(said()).toContain("1 part was left as it was because the model changed too much.");
    expect(said()).toContain("2 suggestions no longer apply because the text changed.");
    expect(button("cancel")).toBeNull();
    expect(button("acceptAll")?.disabled).toBe(false);
    show(review({}));
    expect(said()).toContain("No mistakes found.");
  });

  it("stopped: what was found so far", () => {
    show(review({ status: { kind: "cancelled" }, suggestions: [S1] }));
    expect(said()).toContain("Stopped. 1 suggestion found so far.");
  });

  it("failed: its words, Try again, Local AI settings", () => {
    const error = (subject: string, detail: string): ApiError => ({
      kind: "backend",
      command: "plugin_call",
      error: { kind: "unavailable", subject, detail },
    });
    const retry = vi.spyOn(correction, "retry").mockResolvedValue();
    const open = vi.spyOn(ui, "openSettingsPage").mockImplementation(() => {});
    show(review({ status: { kind: "failed", error: error("model", "noModel") } }));
    const alert = target.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain(
      "The model chosen for correction isn't available. Choose one on the Local AI page.",
    );
    const [again, settings] = [...(alert?.querySelectorAll("button") ?? [])];
    again?.click();
    settings?.click();
    expect(retry).toHaveBeenCalledOnce();
    expect(open).toHaveBeenCalledWith("local-ai.page", "content");
    show(review({ status: { kind: "failed", error: error("engine", "notAvailable") } }));
    expect(said()).toContain("The AI engine isn't available for this computer yet.");
    expect(said()).not.toContain("Get it with Test");
    show(review({ status: { kind: "failed", error: error("engine", "outOfMemory") } }));
    expect(said()).not.toContain("isn't available for this computer");
  });

  it("keys inside the bar: A, R, Shift+A, Shift+R, Esc; never while composing", () => {
    const calls: string[] = [];
    for (const name of ["accept", "reject", "acceptAll", "rejectAll", "done"] as const) {
      vi.spyOn(correction, name).mockImplementation(() => void calls.push(name));
    }
    show(review({ suggestions: [S1, S2] }));
    expect(key({ key: "a" }).defaultPrevented).toBe(true);
    key({ key: "r" });
    key({ key: "A", shiftKey: true });
    key({ key: "R", shiftKey: true });
    key({ key: "a", isComposing: true });
    key({ key: "a", ctrlKey: true });
    key({ key: "Escape" });
    expect(calls).toEqual(["accept", "reject", "acceptAll", "rejectAll", "done"]);
  });

  it("one live region: the phase, then the count and the current step at the end", () => {
    show(review({ status: { kind: "loading", progress: 0.2 } }));
    expect(target.querySelectorAll("[aria-live]")).toHaveLength(1);
    expect(target.querySelector("[aria-live]")?.textContent).toContain("Getting the model ready…");
    show(review({ suggestions: [S1, S2] }));
    const live = target.querySelector("[aria-live]")?.textContent ?? "";
    expect(live).toContain("2 suggestions");
    expect(live).toContain("Suggestion 1 of 2");
  });

  it("the focused button turned off: the keyboard goes to Cancel while running, else Done", () => {
    show(review({ status: { kind: "checking", part: 1, parts: 2 }, suggestions: [S1] }));
    button("accept")?.focus();
    show(review({ status: { kind: "checking", part: 2, parts: 2 }, suggestions: [] }));
    expect(document.activeElement).toBe(button("cancel"));
    show(review({ suggestions: [S1] }));
    button("reject")?.focus();
    show(review({ suggestions: [], outdated: 1 }));
    expect(document.activeElement).toBe(button("done"));
  });

  it("asked for the keyboard (Mod+Alt+K in review): Accept takes it", () => {
    show(review({ suggestions: [S1] }));
    correction.focusRequest += 1;
    flushSync();
    expect(document.activeElement).toBe(button("accept"));
  });
});
