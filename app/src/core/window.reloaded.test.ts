// A view whose page process ended is loaded again by the backend: its next boot says so, and the notice names the
// document whose unsaved edits went with the page (the backend never holds unsaved text, so nothing can bring them
// back).
import { describe, expect, it, vi } from "vitest";
import { createMockApi } from "../api/mock";
import { t } from "./i18n";
import { WindowStore, type Notice } from "./window.svelte";

type Mock = ReturnType<typeof createMockApi>;
type ActionNotice = Extract<Notice, { kind: "action" }>;

async function started(api: Mock): Promise<WindowStore> {
  const store = new WindowStore(api);
  await store.start();
  return store;
}

/** A mock whose view is a document tab (the mock boots as the window's Welcome otherwise). */
async function inTab(): Promise<Mock> {
  const api = createMockApi();
  const opened = await api.openPath("docs/guide.md", "tab");
  if (!opened.ok) throw new Error("no tab");
  return api;
}

function reloadedNotices(store: WindowStore): ActionNotice[] {
  return store.notices.filter(
    (n): n is ActionNotice => n.kind === "action" && n.key === "reloaded",
  );
}

describe("a view loaded again after its page process ended", () => {
  it("not reloaded: no notice", async () => {
    const store = await started(createMockApi());
    expect(reloadedNotices(store)).toEqual([]);
  });

  it("unsaved edits went with the page: one notice names the document and waits for the person", async () => {
    vi.useFakeTimers();
    try {
      const api = await inTab();
      api.probe.reloaded = { lost: "docs/guide.md" };
      const store = await started(api);
      const notices = reloadedNotices(store);
      expect(notices).toHaveLength(1);
      expect(notices[0]?.text).toBe(t("stuck.reloadedLost", { file: "docs/guide.md" }));
      expect(notices[0]?.lasting).toBe(true);
      vi.advanceTimersByTime(60_000);
      expect(reloadedNotices(store)).toHaveLength(1);
      // told once: the next boot of the view (a later load of its page) says nothing
      const again = await started(api);
      expect(reloadedNotices(again)).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("nothing unsaved was lost: a quiet note that the tab was loaded again, gone by itself", async () => {
    vi.useFakeTimers();
    try {
      const api = await inTab();
      api.probe.reloaded = { lost: null };
      const store = await started(api);
      expect(store.tab).not.toBeNull();
      const notices = reloadedNotices(store);
      expect(notices.map((n) => n.text)).toEqual([t("stuck.reloaded")]);
      expect(notices[0]?.lasting).not.toBe(true);
      vi.advanceTimersByTime(60_000);
      expect(reloadedNotices(store)).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("the window's Welcome view (no tab) is a page, not a tab: the note says so", async () => {
    const api = createMockApi();
    api.probe.reloaded = { lost: null };
    const store = await started(api);
    expect(store.tab).toBeNull();
    expect(reloadedNotices(store).map((n) => n.text)).toEqual([t("stuck.reloadedPage")]);
    expect(t("stuck.reloadedPage")).not.toMatch(/tab/i);
  });
});
