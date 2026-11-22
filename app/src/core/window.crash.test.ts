// The last run's crash report, offered at start: one notice that waits for the person. Nothing leaves the computer
// unless the person copies the report or pastes it into the issue page opened in the browser.
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, MOCK_CRASH } from "../api/mock";
import { t } from "./i18n";
import { WindowStore, type Notice } from "./window.svelte";

type Mock = ReturnType<typeof createMockApi>;
type ActionNotice = Extract<Notice, { kind: "action" }>;

function crashed(): Mock {
  const api = createMockApi();
  api.probe.crash.offer = MOCK_CRASH;
  return api;
}

async function started(api: Mock): Promise<WindowStore> {
  const store = new WindowStore(api);
  await store.start();
  return store;
}

function crashNotice(store: WindowStore): ActionNotice | undefined {
  return store.notices.find(
    (n): n is ActionNotice => n.kind === "action" && n.text === t("crash.notice"),
  );
}

/** As `Notices.svelte` does: the notice goes ("action"), then its button's work runs. */
async function press(store: WindowStore, label: string): Promise<void> {
  const notice = crashNotice(store);
  const action = notice?.actions.find((a) => a.label === label);
  if (!notice || !action) throw new Error(`no crash notice button ${label}`);
  store.dismiss(notice.id, "action");
  await action.run();
}

function notes(store: WindowStore): string[] {
  return store.notices.flatMap((n) => (n.kind === "note" ? [n.text] : []));
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("a crash report from the last run", () => {
  it("none: no notice", async () => {
    const api = createMockApi();
    const store = await started(api);
    expect(crashNotice(store)).toBeUndefined();
    expect(store.notices).toEqual([]);
  });

  it("offered once: one lasting notice with Copy report, Report on GitHub and Open folder", async () => {
    vi.useFakeTimers();
    try {
      const api = crashed();
      const store = await started(api);
      const notice = crashNotice(store);
      expect(notice?.lasting).toBe(true);
      expect(notice?.key).toBe("crash");
      expect(notice?.actions.map((a) => a.label)).toEqual([
        t("crash.copy"),
        t("crash.report"),
        t("crash.openFolder"),
      ]);
      // it waits for the person: no clock takes it away
      vi.advanceTimersByTime(60_000);
      expect(crashNotice(store)).toBeDefined();
      // the backend hands it to the first window only: the next one that boots shows none
      const other = await started(api);
      expect(crashNotice(other)).toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
  });

  it("Copy report puts the whole report on the clipboard, says so, and the offer is not made again", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const api = crashed();
    const store = await started(api);
    await press(store, t("crash.copy"));
    expect(writeText).toHaveBeenCalledWith(MOCK_CRASH.report);
    expect(notes(store)).toEqual([t("edit.copied")]);
    expect(api.probe.crash.dismissed).toBe(1);
    expect(crashNotice(store)).toBeUndefined();
  });

  it("a clipboard that refuses: says so and offers the report again, still unseen", async () => {
    vi.stubGlobal("navigator", {
      clipboard: { writeText: () => Promise.reject(new Error("denied")) },
    });
    const api = crashed();
    const store = await started(api);
    await press(store, t("crash.copy"));
    expect(notes(store)).toEqual([t("doc.copyFailed")]);
    expect(crashNotice(store)).toBeDefined();
    expect(api.probe.crash.dismissed).toBe(0);
  });

  it("Report on GitHub copies the whole report, then opens the issue the backend built (the person pastes and submits)", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const api = crashed();
    const store = await started(api);
    await press(store, t("crash.report"));
    expect(writeText).toHaveBeenCalledWith(MOCK_CRASH.report);
    expect(notes(store)).toEqual([t("edit.copied")]);
    expect(api.probe.web).toEqual([MOCK_CRASH.issueUrl]);
    expect(api.probe.crash.dismissed).toBe(1);
    expect(crashNotice(store)).toBeUndefined();
  });

  it("Report on GitHub with a clipboard that refuses: still opens the issue, says so and offers the report again", async () => {
    vi.stubGlobal("navigator", {
      clipboard: { writeText: () => Promise.reject(new Error("denied")) },
    });
    const api = crashed();
    const store = await started(api);
    await press(store, t("crash.report"));
    expect(api.probe.web).toEqual([MOCK_CRASH.issueUrl]);
    expect(notes(store)).toEqual([t("doc.copyFailed")]);
    // the page asks for the report: Copy report and Open folder stay at hand
    expect(crashNotice(store)).toBeDefined();
    expect(api.probe.crash.dismissed).toBe(0);
  });

  it("Open folder shows the report's file; a refusal says why and offers the report again", async () => {
    const api = crashed();
    const store = await started(api);
    api.probe.crash.failReveal = "io";
    await press(store, t("crash.openFolder"));
    expect(store.notices.some((n) => n.kind === "apiFailed")).toBe(true);
    expect(crashNotice(store)).toBeDefined();
    expect(api.probe.crash.dismissed).toBe(0);
    await press(store, t("crash.openFolder"));
    expect(api.probe.crash.revealed).toBe(1);
    expect(api.probe.crash.dismissed).toBe(1);
  });

  it("Dismiss: not offered again", async () => {
    const api = crashed();
    const store = await started(api);
    const notice = crashNotice(store);
    if (!notice) throw new Error("no crash notice");
    store.dismiss(notice.id);
    expect(api.probe.crash.dismissed).toBe(1);
  });

  it("pushed out by newer notices before the person saw it: offered again at the next start", async () => {
    const api = crashed();
    const store = await started(api);
    for (const text of ["one", "two", "three"]) store.notifyAction(text);
    expect(crashNotice(store)).toBeUndefined();
    expect(api.probe.crash.dismissed).toBe(0);
  });
});
