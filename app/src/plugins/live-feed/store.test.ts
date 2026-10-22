// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FeedItem } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { FeedStore, TOAST_MS, isDocItem, isDocument, mergeItems } from "./store.svelte";
import { documents } from "../../core/doc/document.svelte";
import { mark, markLines } from "./glow";
import { silence } from "../../core/silence/silence.svelte";
import { entry } from "./testing";
import { ago, split } from "./time";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

async function setup(shown: string | null = null) {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  const store = new FeedStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    () => shown,
    api,
    () => false,
  );
  const stop = store.start();
  await settle();
  return { api, store, stop };
}

afterEach(() => vi.useRealTimers());

describe("feed items", () => {
  it("merge newest first, an updated item replaces its older self, and the list is capped", () => {
    const merged = mergeItems(
      [entry(1, "a.md"), entry(2, "b.md")],
      [entry(1, "a.md", "modified", { saves: 4, atMs: 5000 })],
    );
    expect(merged.map((i) => i.id)).toEqual([1, 2]);
    expect(merged[0]).toMatchObject({ saves: 4 });
    const many = Array.from({ length: 600 }, (_, n) => entry(n + 1, `f${n}.md`));
    expect(mergeItems([], many)).toHaveLength(500);
    expect(mergeItems([], many)[0]?.id).toBe(600);
  });

  it("'Docs only' keeps documents and groups, and says what it hides", () => {
    expect(isDocItem(entry(1, "a.md"))).toBe(true);
    expect(isDocItem(entry(2, "notes.txt"))).toBe(true);
    expect(isDocItem(entry(3, "data.json"))).toBe(false);
    expect(isDocItem(entry(4, "docs", "modified", { isDir: true }))).toBe(false);
    expect(
      isDocItem({ item: "group", id: 5, reason: "git", count: 3, atMs: 1, sample: [], own: false }),
    ).toBe(true);
    expect(isDocument("x/y.MD")).toBe(true);
    expect(isDocument("x/y.txt")).toBe(false);
  });

  it("times and names in words", () => {
    expect(split("a/b/c.md")).toEqual({ name: "c.md", folder: "a/b" });
    expect(split("c.md")).toEqual({ name: "c.md", folder: "" });
    expect(ago(0, 30_000)).toMatch(/30/);
    expect(ago(10_000, 5_000)).toMatch(/0/);
  });
});

afterEach(() => silence.clear());

describe("the feed store", () => {
  it("lists what the backend has and what it reports, and filters", async () => {
    const { api, store, stop } = await setup();
    expect(store.items).toHaveLength(2);
    api.probe.plugins.feed([entry(10, "data.json", "modified", { atMs: Date.now() })]);
    expect(store.items[0]?.id).toBe(10);
    expect(store.visible).toHaveLength(3);
    store.docsOnly = true;
    expect(store.visible).toHaveLength(2);
    expect(store.hidden).toBe(1);
    await store.clear();
    expect(store.items).toHaveLength(0);
    stop();
  });

  it("says documents changed behind the person's back, once, with a count, and goes away by itself", async () => {
    const { api, store, stop } = await setup("shown.md");
    vi.useFakeTimers();
    api.probe.plugins.feed([entry(20, "plan.md")]);
    expect(store.toast).toMatchObject({ count: 1, path: "plan.md", kind: "modified" });
    api.probe.plugins.feed([entry(21, "spec.md", "created")]);
    expect(store.toast).toMatchObject({ count: 2, path: "spec.md", kind: "created" });
    vi.advanceTimersByTime(TOAST_MS + 10);
    expect(store.toast).toBeNull();
    stop();
  });

  it("says nothing for the person's own doing, folders, other files or the document being read", async () => {
    const { api, store, stop } = await setup("shown.md");
    api.probe.plugins.feed([
      entry(30, "mine.md", "modified", { own: true }),
      entry(31, "folder.md", "modified", { isDir: true }),
      entry(32, "data.json"),
      entry(33, "shown.md"),
    ]);
    expect(store.toast).toBeNull();
    store.dismissToast();
    stop();
  });

  it("says nothing of what the person silenced, and leaves it out of the list", async () => {
    const { api, store, stop } = await setup("shown.md");
    silence.set({ quiet: "quiet" });
    api.probe.plugins.feed([entry(40, "quiet/a.md"), entry(41, "loud/b.md")]);
    expect(store.toast).toMatchObject({ count: 1, path: "loud/b.md" });
    expect(store.visible.map((i) => (i.item === "entry" ? i.path : ""))).not.toContain(
      "quiet/a.md",
    );
    expect(store.items.some((i) => i.item === "entry" && i.path === "quiet/a.md")).toBe(true);
    store.docsOnly = true;
    expect(store.hidden).toBe(0);
    // listening again brings it back
    silence.set({});
    expect(store.visible.some((i) => i.item === "entry" && i.path === "quiet/a.md")).toBe(true);
    store.dismissToast();
    stop();
  });

  it("is empty without a project and after teardown", async () => {
    const api = createMockApi();
    const store = new FeedStore(
      new PluginClient(api, () => null),
      () => null,
      () => null,
      api,
      () => false,
    );
    const stop = store.start();
    await settle();
    expect(store.items).toHaveLength(0);
    stop();
  });
});

describe("the glow", () => {
  it("tints new blocks for a moment and leaves a marker where one was removed", () => {
    vi.useFakeTimers();
    const container = document.createElement("div");
    container.innerHTML = '<p id="a"></p><p id="b"></p><p id="c"></p>';
    const element = (id: string): HTMLElement | undefined =>
      container.querySelector<HTMLElement>(`#${id}`) ?? undefined;
    const timers = new Set<ReturnType<typeof setTimeout>>();
    mark({ added: ["b"], removed: [{ id: "x", after: "a" }] }, container, element, timers);
    expect(element("b")?.classList.contains("pm-glow")).toBe(true);
    expect(container.querySelectorAll(".pm-removed-mark")).toHaveLength(1);
    expect(element("a")?.nextElementSibling?.classList.contains("pm-removed-mark")).toBe(true);
    vi.advanceTimersByTime(3100);
    expect(element("b")?.classList.contains("pm-glow")).toBe(false);
    // a removal at the top, a click on the marker
    mark({ added: [], removed: [{ id: "y", after: null }] }, container, element, timers);
    const marker = container.firstElementChild;
    expect(marker?.classList.contains("pm-removed-mark")).toBe(true);
    marker?.dispatchEvent(new Event("click"));
    expect(container.querySelectorAll(".pm-removed-mark")).toHaveLength(0);
    // no page: nothing happens
    mark({ added: ["b"], removed: [] }, null, element, timers);
  });

  it("lights the changed lines of a text for 3 s and keeps the mark where lines went for 8", () => {
    vi.useFakeTimers();
    const timers = new Set<ReturnType<typeof setTimeout>>();
    markLines({ path: "a.rs", added: [{ first: 2, last: 3 }], removedAfter: [7] }, timers);
    expect(documents.lineGlow).toEqual({
      path: "a.rs",
      added: [{ first: 2, last: 3 }],
      removedAfter: [7],
    });
    vi.advanceTimersByTime(3100);
    expect(documents.lineGlow).toEqual({ path: "a.rs", added: [], removedAfter: [7] });
    vi.advanceTimersByTime(5000);
    expect(documents.lineGlow).toBeNull();
    expect(timers.size).toBe(0);
  });

  it("lets a newer change of a text take the place of the one on screen", () => {
    vi.useFakeTimers();
    const timers = new Set<ReturnType<typeof setTimeout>>();
    markLines({ path: "a.rs", added: [{ first: 1, last: 1 }], removedAfter: [] }, timers);
    vi.advanceTimersByTime(2000);
    markLines({ path: "a.rs", added: [{ first: 5, last: 6 }], removedAfter: [] }, timers);
    vi.advanceTimersByTime(2000);
    // the first one's 3 s are over, the second one's are not
    expect(documents.lineGlow?.added).toEqual([{ first: 5, last: 6 }]);
    vi.advanceTimersByTime(1100);
    expect(documents.lineGlow?.added).toEqual([]);
    vi.advanceTimersByTime(9000);
    expect(documents.lineGlow).toBeNull();
    documents.lineGlow = null;
  });
});
