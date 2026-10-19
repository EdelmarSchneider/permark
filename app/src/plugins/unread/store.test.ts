// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { Box } from "../../core/testing/box.svelte";
import { silence } from "../../core/silence/silence.svelte";
import { READ_MS, UnreadStore, isDocument, nextUnread } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

type Shown = { path: string | null; hash: string | null };

async function setup(shown: Shown = { path: null, hash: null }) {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisWindow");
  const current = new Box<Shown>(shown);
  const store = new UnreadStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    () => current.value,
  );
  const stop = store.start();
  await settle();
  return {
    api,
    store,
    stop,
    show(next: Shown) {
      current.value = next;
      flushSync();
    },
  };
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  silence.clear();
});

describe("next unread", () => {
  it("is the one after the document, wrapping, never the document itself", () => {
    const list = ["a.md", "b/c.md", "d.md"];
    expect(nextUnread(list, null)).toBe("a.md");
    expect(nextUnread(list, "a.md")).toBe("b/c.md");
    expect(nextUnread(list, "b/c.md")).toBe("d.md");
    expect(nextUnread(list, "d.md")).toBe("a.md");
    expect(nextUnread(list, "bz.md")).toBe("d.md");
    expect(nextUnread(["a.md"], "a.md")).toBeNull();
    expect(nextUnread([], null)).toBeNull();
    expect(isDocument("x.markdown")).toBe(true);
    expect(isDocument("x.txt")).toBe(false);
  });
});

describe("the unread store", () => {
  it("puts a dot on unread documents and a count on the folders above", async () => {
    const { store, api, stop } = await setup();
    expect(store.ready).toBe(true);
    expect(store.badge("docs/guide.md", false)).toMatchObject({ text: "•", tone: "unread" });
    expect(store.badge("README.md", false)).toBeNull();
    expect(store.badge("docs", true)).toMatchObject({ text: "1", tone: "unread" });
    expect(store.countBelow("docs")).toBe(1);
    api.probe.plugins.unread(["docs/other.md", "docs/deep/x.md"]);
    expect(store.badge("docs", true)).toMatchObject({ text: "3" });
    expect(store.badge("docs/deep", true)).toMatchObject({ text: "1" });
    expect(store.next("docs/deep/x.md")).toBe("docs/guide.md");
    stop();
  });

  it("marks documents read, unread, a folder, everything", async () => {
    const { store, api, stop } = await setup();
    api.probe.plugins.unread(["docs/other.md", "notes/n.md"]);
    await store.markRead("docs/guide.md", "0".repeat(16));
    expect(store.isUnread("docs/guide.md")).toBe(false);
    await store.markFolderRead("docs");
    expect(store.unread).toEqual(["notes/n.md"]);
    await store.markUnread("README.md");
    expect(store.isUnread("README.md")).toBe(true);
    await store.markFolderRead("");
    expect(store.unread).toEqual([]);
    stop();
  });

  it("leaves what the person silenced out of the dots, counts and the list, and starts it from now when it speaks again", async () => {
    const { store, api, stop } = await setup();
    api.probe.plugins.unread(["docs/other.md", "notes/n.md"]);
    silence.set({ docs: "quiet" });
    expect(store.badge("docs/guide.md", false)).toBeNull();
    expect(store.badge("docs", true)).toBeNull();
    expect(store.countBelow("docs")).toBe(0);
    expect(store.unread).toEqual(["notes/n.md"]);
    expect(store.next(null)).toBe("notes/n.md");
    // reading a silenced document still counts as reading it
    expect(store.isUnread("docs/guide.md")).toBe(true);
    // it speaks again: what changed meanwhile is marked read, not delivered at once
    silence.set({});
    await vi.waitFor(() => expect(store.isUnread("docs/guide.md")).toBe(false));
    expect(store.isUnread("docs/other.md")).toBe(false);
    expect(store.isUnread("notes/n.md")).toBe(true);
    stop();
  });

  it("loads what changed since the last visit once the first look is done", async () => {
    const { store, stop } = await setup();
    expect(store.since?.ready).toBe(true);
    expect(store.since?.items.map((i) => i.kind)).toEqual(["changed", "new"]);
    stop();
    expect(store.since).toBeNull();
  });

  it("counts a document as read after it has been in view for a moment, and only then", async () => {
    // a window in front: visible, with the focus
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    const { store, api, stop, show } = await setup();
    vi.useFakeTimers();
    show({ path: "docs/guide.md", hash: "ab".repeat(8) });
    vi.advanceTimersByTime(READ_MS - 100);
    expect(store.isUnread("docs/guide.md")).toBe(true);
    // the person moves on before the moment is over: it stays unread
    show({ path: "README.md", hash: "cd".repeat(8) });
    vi.advanceTimersByTime(READ_MS + 100);
    expect(store.isUnread("docs/guide.md")).toBe(true);
    // back, and staying
    show({ path: "docs/guide.md", hash: "ab".repeat(8) });
    vi.advanceTimersByTime(READ_MS + 10);
    await vi.advanceTimersByTimeAsync(0);
    expect(api.probe.plugins.calls).toContain("unread.markRead");
    expect(store.isUnread("docs/guide.md")).toBe(false);
    stop();
  });

  it("does not count reading while the window is not in front", async () => {
    const { store, stop, show } = await setup();
    vi.useFakeTimers();
    window.dispatchEvent(new Event("blur"));
    flushSync();
    // jsdom's document has no focus: the store sees a window that is not in front
    show({ path: "docs/guide.md", hash: "ab".repeat(8) });
    vi.advanceTimersByTime(READ_MS * 3);
    await vi.advanceTimersByTimeAsync(0);
    expect(store.isUnread("docs/guide.md")).toBe(true);
    stop();
  });
});
