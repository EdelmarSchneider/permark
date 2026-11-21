// @vitest-environment jsdom
// The whole window on the mock backend. A close | tab-close ask that reaches this view is acknowledged at once: the
// backend times only how long the view takes to hear it (a dead | hung page gets the not-responding dialog), never how
// long the person thinks about the prompt. The system dialogs' words follow the interface language.
import { flushSync, mount, unmount } from "svelte";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};

const { api } = await import("../../api");
const { windowStore: store } = await import("../window.svelte");
const { default: App } = await import("../../App.svelte");
const { ui } = await import("./ui.svelte");
const { language } = await import("../i18n");
const { nativeWords } = await import("./nativeWords");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

let app: Record<string, unknown> | null = null;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

beforeAll(async () => {
  await store.start();
  const target = document.createElement("div");
  document.body.append(target);
  app = mount(App, { target });
  flushSync();
  // the listeners are up once the mock has them
  await new Promise((r) => setTimeout(r, 0));
});

afterEach(() => {
  ui.closePrompt = null;
});

afterAll(async () => {
  if (app) await unmount(app);
  app = null;
  await language.choose("en", []);
});

describe("asks that reach this view", () => {
  it("the window close prompt: heard at once, while the prompt waits for the person", async () => {
    const before = mock.probe.tabs.heard;
    mock.emit("close-requested", {
      window: store.label,
      tabs: [{ tab: 1, title: "guide.md", document: "docs/guide.md" }],
    });
    await until(() => mock.probe.tabs.heard === before + 1, "the ask was heard");
    expect(ui.closePrompt).not.toBeNull();
  });

  it("an ask for another view is not this view's to hear", async () => {
    const before = mock.probe.tabs.heard;
    mock.emit("close-requested", {
      window: "w-other",
      tabs: [{ tab: 1, title: "guide.md", document: "docs/guide.md" }],
    });
    mock.emit("tab-close-requested", { window: "w-other" });
    await new Promise((r) => setTimeout(r, 20));
    expect(mock.probe.tabs.heard).toBe(before);
    expect(ui.closePrompt).toBeNull();
  });

  it("the unsaved prompt before this tab closes: heard at once", async () => {
    const before = mock.probe.tabs.heard;
    mock.emit("tab-close-requested", { window: store.label });
    await until(() => mock.probe.tabs.heard === before + 1, "the ask was heard");
  });

  it("Save all: the save's answer is what the backend waits for", async () => {
    const heard = mock.probe.tabs.heard;
    const saved = mock.probe.tabs.saved.length;
    mock.emit("save-requested", { window: store.label });
    await until(() => mock.probe.tabs.saved.length === saved + 1, "the save answered");
    expect(mock.probe.tabs.heard).toBe(heard);
  });
});

describe("the words of the system dialogs", () => {
  it("sent in the interface language, and again when the language changes", async () => {
    await until(
      () => mock.probe.nativeWords.at(-1)?.closeAnyway === nativeWords().closeAnyway,
      "the English words were sent",
    );
    expect(mock.probe.nativeWords.at(-1)).toEqual(nativeWords());
    await language.choose("fr", []);
    await until(
      () => mock.probe.nativeWords.at(-1)?.closeAnyway === "Fermer quand même",
      "the French words were sent",
    );
    expect(mock.probe.nativeWords.at(-1)).toEqual(nativeWords());
  });
});
