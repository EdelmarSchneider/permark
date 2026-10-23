// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { Box } from "../../core/testing/box.svelte";
import { silence } from "../../core/silence/silence.svelte";
import { READ_MS } from "../../core/readRule";
import { UnreadStore, isDocument, nextUnread } from "./store.svelte";

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
  await api.openPath(MOCK_FOLDER, "thisTab");
  const current = new Box<Shown>(shown);
  /** This view is its window's active tab. */
  const onScreen = new Box(true);
  const store = new UnreadStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    () => current.value,
    () => onScreen.value,
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
    /** The person switches to another tab of the window (`false`) or back (`true`). */
    tab(on: boolean) {
      onScreen.value = on;
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
  it("is the one after the document in the list (newest change first), wrapping, never the document itself", () => {
    // the backend's order: newest change first
    const list = ["d.md", "a.md", "b/c.md"];
    expect(nextUnread(list, null)).toBe("d.md");
    expect(nextUnread(list, "d.md")).toBe("a.md");
    expect(nextUnread(list, "a.md")).toBe("b/c.md");
    expect(nextUnread(list, "b/c.md")).toBe("d.md");
    // a document that is read (not in the list): the newest change
    expect(nextUnread(list, "bz.md")).toBe("d.md");
    expect(nextUnread(["a.md"], "a.md")).toBeNull();
    expect(nextUnread(["a.md"], "x.md")).toBe("a.md");
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
    // newest change first: the last one the agent wrote, then the one before
    expect(store.unread).toEqual(["docs/deep/x.md", "docs/other.md", "docs/guide.md"]);
    expect(store.next(null)).toBe("docs/deep/x.md");
    expect(store.next("docs/deep/x.md")).toBe("docs/other.md");
    expect(store.next("docs/guide.md")).toBe("docs/deep/x.md");
    stop();
  });

  it("one project's documents: its count for the detail card, the list the tab shows (UX round 4 F8)", async () => {
    const { api, store, stop } = await setup();
    api.probe.plugins.unread(["hub-b/a.md", "hub-b/x/b.md", "hub-bc/c.md", "site/d.md"]);
    await settle();
    expect(store.projectCount("hub-b")).toBe("2 unread");
    expect(store.projectCount("tools")).toBeNull();
    expect(store.shown).toEqual(store.unread);
    store.showOnly({ folder: "hub-b", name: "Hub B" });
    flushSync();
    // a sibling folder whose name starts the same is not inside
    expect(store.shown).toEqual(["hub-b/x/b.md", "hub-b/a.md"]);
    store.showOnly(null);
    flushSync();
    expect(store.shown).toEqual(store.unread);
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

  // a tab behind another stays "visible" and may keep the focus (child webviews): only the tab list tells
  it("never counts reading in a tab that is not on screen, even with the focus", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    const { store, api, stop, show, tab } = await setup();
    vi.useFakeTimers();
    show({ path: "docs/guide.md", hash: "ab".repeat(8) });
    vi.advanceTimersByTime(READ_MS - 100);
    // the person switches tabs while the moment runs
    tab(false);
    vi.advanceTimersByTime(READ_MS * 3);
    await vi.advanceTimersByTimeAsync(0);
    expect(store.isUnread("docs/guide.md")).toBe(true);
    // an agent rewrites the document while its tab is hidden: the view reloads, still nobody reads it
    show({ path: "docs/guide.md", hash: "ef".repeat(8) });
    vi.advanceTimersByTime(READ_MS * 3);
    await vi.advanceTimersByTimeAsync(0);
    expect(api.probe.plugins.calls).not.toContain("unread.markRead");
    expect(store.isUnread("docs/guide.md")).toBe(true);
    // back on the tab, and staying: now it is read
    tab(true);
    vi.advanceTimersByTime(READ_MS + 10);
    await vi.advanceTimersByTimeAsync(0);
    expect(store.isUnread("docs/guide.md")).toBe(false);
    stop();
  });

  it("a rapid back and forth between tabs restarts the moment: a sum of glimpses is not a read", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    const { store, stop, show, tab } = await setup();
    vi.useFakeTimers();
    show({ path: "docs/guide.md", hash: "ab".repeat(8) });
    for (let i = 0; i < 5; i++) {
      vi.advanceTimersByTime(READ_MS / 2);
      tab(false);
      tab(true);
    }
    await vi.advanceTimersByTimeAsync(0);
    expect(store.isUnread("docs/guide.md")).toBe(true);
    stop();
  });

  it("reads after 3 s in front, not a moment sooner (UX regressions #7)", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    const { store, stop, show } = await setup();
    vi.useFakeTimers();
    expect(READ_MS).toBe(3000);
    show({ path: "docs/guide.md", hash: "ab".repeat(8) });
    vi.advanceTimersByTime(2900);
    await vi.advanceTimersByTimeAsync(0);
    expect(store.isUnread("docs/guide.md")).toBe(true);
    vi.advanceTimersByTime(100);
    await vi.advanceTimersByTimeAsync(0);
    expect(store.isUnread("docs/guide.md")).toBe(false);
    stop();
  });

  it("a tab switch at 2 s leaves the document unread", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    const { store, stop, show, tab } = await setup();
    vi.useFakeTimers();
    show({ path: "docs/guide.md", hash: "ab".repeat(8) });
    vi.advanceTimersByTime(2000);
    tab(false);
    vi.advanceTimersByTime(READ_MS * 2);
    await vi.advanceTimersByTimeAsync(0);
    expect(store.isUnread("docs/guide.md")).toBe(true);
    stop();
  });

  describe("a scroll of the document", () => {
    const pane = (): HTMLElement => {
      const el = document.createElement("div");
      el.setAttribute("data-scroller", "");
      el.tabIndex = -1;
      el.innerHTML = "<p>text</p>";
      document.body.append(el);
      return el;
    };
    afterEach(() => document.body.replaceChildren());

    it("by the person reads it at once (0.5 s in)", async () => {
      vi.spyOn(document, "hasFocus").mockReturnValue(true);
      vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
      const { store, stop, show } = await setup();
      const el = pane();
      vi.useFakeTimers();
      show({ path: "docs/guide.md", hash: "ab".repeat(8) });
      vi.advanceTimersByTime(500);
      el.querySelector("p")?.dispatchEvent(new WheelEvent("wheel", { bubbles: true, deltaY: 120 }));
      el.dispatchEvent(new Event("scroll"));
      await vi.advanceTimersByTimeAsync(0);
      expect(store.isUnread("docs/guide.md")).toBe(false);
      stop();
    });

    it("by keys on the page counts; keys typed into an editor do not", async () => {
      vi.spyOn(document, "hasFocus").mockReturnValue(true);
      vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
      const { store, stop, show } = await setup();
      const el = pane();
      const editor = document.createElement("div");
      editor.setAttribute("contenteditable", "true");
      el.append(editor);
      vi.useFakeTimers();
      show({ path: "docs/guide.md", hash: "ab".repeat(8) });
      editor.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
      el.dispatchEvent(new Event("scroll"));
      await vi.advanceTimersByTimeAsync(0);
      expect(store.isUnread("docs/guide.md")).toBe(true);
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "PageDown", bubbles: true }));
      el.dispatchEvent(new Event("scroll"));
      await vi.advanceTimersByTimeAsync(0);
      expect(store.isUnread("docs/guide.md")).toBe(false);
      stop();
    });

    it("by the app (a place restored, a jump) or long after the input does not count", async () => {
      vi.spyOn(document, "hasFocus").mockReturnValue(true);
      vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
      const { store, stop, show } = await setup();
      const el = pane();
      vi.useFakeTimers();
      show({ path: "docs/guide.md", hash: "ab".repeat(8) });
      el.dispatchEvent(new Event("scroll"));
      el.dispatchEvent(new WheelEvent("wheel", { bubbles: true }));
      vi.advanceTimersByTime(1500);
      el.dispatchEvent(new Event("scroll"));
      await vi.advanceTimersByTimeAsync(0);
      expect(store.isUnread("docs/guide.md")).toBe(true);
      // a wheel outside the document pane (a sidebar list) is not reading it
      document.body.dispatchEvent(new WheelEvent("wheel", { bubbles: true }));
      el.dispatchEvent(new Event("scroll"));
      await vi.advanceTimersByTimeAsync(0);
      expect(store.isUnread("docs/guide.md")).toBe(true);
      stop();
    });

    it("while the window is not in front does not count", async () => {
      vi.spyOn(document, "hasFocus").mockReturnValue(false);
      const { store, stop, show } = await setup();
      const el = pane();
      vi.useFakeTimers();
      show({ path: "docs/guide.md", hash: "ab".repeat(8) });
      el.dispatchEvent(new WheelEvent("wheel", { bubbles: true }));
      el.dispatchEvent(new Event("scroll"));
      await vi.advanceTimersByTimeAsync(0);
      expect(store.isUnread("docs/guide.md")).toBe(true);
      stop();
    });
  });
});
