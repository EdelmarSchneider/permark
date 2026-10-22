// @vitest-environment jsdom
// The feed present when it matters: dormant until the first outside change (remembered per project), the count of
// outside changes not seen yet, the toast held while the person is on it, the sections of glowing blocks.
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { documents } from "../../core/doc/document.svelte";
import { PluginClient } from "../../core/plugins/client";
import { silence } from "../../core/silence/silence.svelte";
import {
  CHANGED_MS,
  FeedStore,
  OFFERED_KEY,
  TOAST_MS,
  countText,
  isOutsideChange,
} from "./store.svelte";
import { entry } from "./testing";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

async function setup(
  options: {
    shown?: string | null;
    onScreen?: () => boolean;
    quiet?: boolean;
    offered?: boolean;
  } = {},
) {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  // a project nobody changed from outside yet: no history in the feed
  if (options.quiet) await api.pluginCall("live-feed.clear", {});
  if (options.offered) await api.projectSessionSet(OFFERED_KEY, true);
  const store = new FeedStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    () => options.shown ?? null,
    api,
    options.onScreen ?? (() => false),
  );
  const stop = store.start();
  await settle();
  return { api, store, stop };
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  silence.clear();
});

describe("dormant until the project calls for it", () => {
  it("wakes up at the first outside change, not at the person's own, and remembers it", async () => {
    const { api, store, stop } = await setup({ quiet: true });
    expect(store.offered).toBe(false);
    api.probe.plugins.feed([entry(50, "mine.md", "modified", { own: true })]);
    expect(store.offered).toBe(false);
    api.probe.plugins.feed([entry(51, "plan.md")]);
    expect(store.offered).toBe(true);
    expect(await api.projectSessionGet(OFFERED_KEY)).toEqual({ ok: true, value: true });
    stop();
  });

  it("wakes up from outside changes already in the feed, or from what the project remembers", async () => {
    const history = await setup();
    expect(history.store.offered).toBe(true);
    history.stop();
    const remembered = await setup({ quiet: true, offered: true });
    expect(remembered.store.offered).toBe(true);
    remembered.stop();
  });

  it("is not woken by silenced files, own groups or a resync; a foreign group wakes it", async () => {
    silence.set({ quiet: "quiet" });
    const { api, store, stop } = await setup({ quiet: true });
    api.probe.plugins.feed([entry(52, "quiet/a.md")]);
    api.probe.plugins.feed([
      { item: "group", id: 53, reason: "large", count: 9, atMs: 1, sample: [], own: true },
    ]);
    api.probe.plugins.feed([{ item: "resync", id: 54, atMs: 2 }]);
    expect(store.offered).toBe(false);
    api.probe.plugins.feed([
      { item: "group", id: 55, reason: "git", count: 3, atMs: 3, sample: [], own: false },
    ]);
    expect(store.offered).toBe(true);
    stop();
  });

  it("wakes up when the person asks for the panel", async () => {
    const { api, store, stop } = await setup({ quiet: true });
    store.toggle();
    expect(store.offered).toBe(true);
    expect(await api.projectSessionGet(OFFERED_KEY)).toEqual({ ok: true, value: true });
    stop();
  });

  it("knows an outside change", () => {
    const none = (): boolean => false;
    expect(isOutsideChange(entry(1, "a.md"), none)).toBe(true);
    expect(isOutsideChange(entry(1, "a.md", "modified", { own: true }), none)).toBe(false);
    expect(isOutsideChange(entry(1, "a.md"), () => true)).toBe(false);
    expect(isOutsideChange({ item: "resync", id: 2, atMs: 1 }, none)).toBe(false);
  });
});

describe("outside changes not seen yet", () => {
  it("counts new outside rows while the panel is not on screen, never history, and clears on screen", async () => {
    const screen = $state({ on: false });
    const { api, store, stop } = await setup({ shown: "shown.md", onScreen: () => screen.on });
    expect(store.unseen).toBe(0);
    api.probe.plugins.feed([entry(60, "a.md"), entry(61, "b.json")]);
    expect(store.unseen).toBe(2);
    // a coalesced update of a row already listed is no new row; own changes are not outside
    api.probe.plugins.feed([entry(60, "a.md", "modified", { saves: 2 })]);
    api.probe.plugins.feed([entry(62, "c.md", "modified", { own: true })]);
    expect(store.unseen).toBe(2);
    // "Docs only" leaves other files out of the count, as out of the list
    store.docsOnly = true;
    api.probe.plugins.feed([entry(63, "d.json"), entry(64, "e.md")]);
    expect(store.unseen).toBe(3);
    screen.on = true;
    flushSync();
    expect(store.unseen).toBe(0);
    api.probe.plugins.feed([entry(65, "f.md")]);
    expect(store.unseen).toBe(0);
    stop();
  });

  it("leaves silenced files out of the count", async () => {
    silence.set({ quiet: "quiet" });
    const { api, store, stop } = await setup();
    api.probe.plugins.feed([entry(66, "quiet/a.md"), entry(67, "loud/b.md")]);
    expect(store.unseen).toBe(1);
    stop();
  });

  it("shows 99+ above 99", () => {
    expect(countText(3)).toBe("3");
    expect(countText(99)).toBe("99");
    expect(countText(100)).toBe("99+");
  });
});

describe("the toast, held", () => {
  it("stays while held and gets its full time again after", async () => {
    const { api, store, stop } = await setup({ shown: "shown.md" });
    vi.useFakeTimers();
    api.probe.plugins.feed([entry(70, "plan.md")]);
    store.holdToast();
    vi.advanceTimersByTime(TOAST_MS * 3);
    expect(store.toast).not.toBeNull();
    // more news while held: still held
    api.probe.plugins.feed([entry(71, "spec.md")]);
    vi.advanceTimersByTime(TOAST_MS * 2);
    expect(store.toast).toMatchObject({ count: 2 });
    store.releaseToast();
    vi.advanceTimersByTime(TOAST_MS - 100);
    expect(store.toast).not.toBeNull();
    vi.advanceTimersByTime(200);
    expect(store.toast).toBeNull();
    // dismissed while held: the next one is not held
    api.probe.plugins.feed([entry(72, "next.md")]);
    store.holdToast();
    store.dismissToast();
    api.probe.plugins.feed([entry(73, "last.md")]);
    vi.advanceTimersByTime(TOAST_MS + 10);
    expect(store.toast).toBeNull();
    stop();
  });
});

describe("changed sections", () => {
  it("marks the sections of glowing blocks for as long as they glow, on that document only", async () => {
    const { store, stop } = await setup();
    vi.useFakeTimers();
    const view = {
      path: "doc.md",
      body: {
        kind: "markdown",
        outline: [{ block: "h1" }, { block: "h2" }],
        blocks: [{ id: "h1" }, { id: "p1" }, { id: "h2" }, { id: "p2" }],
      },
    };
    vi.spyOn(documents, "view", "get").mockReturnValue(view as never);
    store.glow("doc.md", ["p2"]);
    expect([...store.changed]).toEqual([1]);
    vi.advanceTimersByTime(CHANGED_MS / 2);
    store.glow("doc.md", ["p1"]);
    expect([...store.changed].sort()).toEqual([0, 1]);
    vi.advanceTimersByTime(CHANGED_MS / 2 + 10);
    expect([...store.changed]).toEqual([0]);
    vi.advanceTimersByTime(CHANGED_MS);
    expect(store.changed.size).toBe(0);
    // another document: nothing marked
    store.glow("other.md", ["p1"]);
    expect(store.changed.size).toBe(0);
    stop();
  });

  it("forgets the offer, the count and the glow when the project goes", async () => {
    const where = $state<{ root: string | null }>({ root: MOCK_FOLDER });
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisTab");
    const store = new FeedStore(
      new PluginClient(api, () => where.root),
      () => where.root,
      () => null,
      api,
      () => false,
    );
    const stop = store.start();
    await settle();
    api.probe.plugins.feed([entry(80, "a.md")]);
    store.glow("a.md", ["x"]);
    expect(store.offered).toBe(true);
    expect(store.unseen).toBe(1);
    where.root = null;
    flushSync();
    expect(store.offered).toBe(false);
    expect(store.unseen).toBe(0);
    expect(store.glowing).toBeNull();
    stop();
  });
});
