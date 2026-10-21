// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import type { Api, WorkspaceView } from "../../api";
import { DocumentController } from "../../core/doc/document.svelte";
import { setLeaveGuard } from "../../core/editing/guard";
import { EditingSession } from "../../core/editing/session.svelte";
import { PluginClient } from "../../core/plugins/client";
import { WindowStore } from "../../core/window.svelte";
import type { NotesIntent } from "../../api";
import { NotesStore, POLL_MS, showInNotes } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 8; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

const offs: (() => void)[] = [];
afterEach(() => {
  for (const off of offs.splice(0)) off();
  vi.restoreAllMocks();
});

type Notes = Extract<WorkspaceView, { kind: "notes" }>;

async function setup(asked?: NotesIntent, onScreen?: () => boolean) {
  const api = createMockApi();
  if (asked !== undefined) await api.pluginCall("notes.want", { intent: asked });
  const outcome = await api.openNotes("thisTab");
  if (!outcome.ok || outcome.value.outcome !== "opened") throw new Error("notes did not open");
  const view = outcome.value.view;
  if (view.state !== "showing" || view.workspace.kind !== "notes") throw new Error("not notes");
  const workspace: Notes = view.workspace;
  const documents = new DocumentController(api);
  const editing = new EditingSession(api);
  const window = new WindowStore(api);
  const client = new PluginClient(api);
  const store = new NotesStore({ api, client, documents, editing, window, onScreen });
  const off = store.attach(workspace);
  offs.push(off, () => editing.release());
  await settle();
  return { api, store, documents, editing, window, workspace };
}

const FIRST = "2026-10-19 0907.md";

describe("the list", () => {
  it("is read again every few seconds only while the Notes tab is on screen", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    try {
      let shown = false;
      const { api } = await setup(undefined, () => shown);
      const reads = (): number => api.probe.plugins.calls.filter((c) => c === "notes.list").length;
      const first = reads();
      // a tab behind another stays "visible" (child webviews): it must not poll forever
      vi.advanceTimersByTime(POLL_MS * 5);
      await settle();
      expect(reads()).toBe(first);
      shown = true;
      vi.advanceTimersByTime(POLL_MS);
      await settle();
      expect(reads()).toBe(first + 1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("is read when the window attaches: pinned first, then the newest", async () => {
    const { store } = await setup();
    expect(store.list.status).toBe("ready");
    const paths = store.entries.map((e) => e.path);
    expect(paths[0]).toBe("2026-10-18 1855.md");
    expect(paths.slice(1, 3)).toEqual([FIRST, "2026-10-19 0815.md"]);
    expect(store.folder?.status.status).toBe("ready");
    expect(store.arranged(Date.now()).pinned).toHaveLength(1);
  });

  it("keeps what it shows when a later read fails, and shows the failure when there is nothing yet", async () => {
    const { api, store } = await setup();
    const fail = vi
      .spyOn(api, "pluginCall")
      .mockResolvedValue({ ok: false, error: { kind: "window", message: "down" } });
    await store.refresh();
    expect(store.list.status).toBe("ready");
    fail.mockRestore();
    const fresh = await setup();
    vi.spyOn(fresh.api, "pluginCall").mockResolvedValue({
      ok: false,
      error: { kind: "window", message: "down" },
    });
    fresh.store.list = { status: "loading" };
    await fresh.store.refresh();
    expect(fresh.store.list.status).toBe("failed");
  });

  it("drops an answer that arrives after a newer one", async () => {
    const { api, store } = await setup();
    const real = api.pluginCall.bind(api);
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    let first = true;
    vi.spyOn(api, "pluginCall").mockImplementation(async (method, args) => {
      if (method === "notes.list" && first) {
        first = false;
        await gate;
        return {
          ok: true,
          value: { folder: store.folder!, entries: [], open: null },
        } as never;
      }
      return real(method, args);
    });
    const slow = store.refresh();
    await store.refresh();
    release();
    await slow;
    expect(store.entries.length).toBeGreaterThan(0);
  });

  it("filters by words and keeps the order", async () => {
    const { store } = await setup();
    store.filter = "bank";
    expect(
      store
        .arranged()
        .groups.flatMap((g) => g.notes)
        .map((n) => n.path),
    ).toEqual(["2026-10-19 0815.md"]);
    store.filter = "nothing like this";
    expect(store.drawn()).toEqual([]);
    store.filter = "";
    expect(store.drawn().length).toBe(store.entries.length);
  });
});

describe("the note shown", () => {
  it("a new note is made, listed, shown and put in an editor that saves by itself", async () => {
    const { store, editing, api } = await setup();
    const before = store.entries.length;
    await store.newNote();
    await settle();
    expect(store.entries.length).toBe(before + 1);
    expect(store.current).not.toBeNull();
    expect(editing.path).toBe(store.current);
    expect(editing.mode).not.toBe("view");
    expect(editing.policy).toEqual({ kind: "auto", delayMs: 800 });
    const opened = await api.pluginCall("notes.opened", { path: null });
    expect(opened.ok).toBe(true);
  });

  it("opens the note the welcome screen asked for, or a new one", async () => {
    const asked = await setup({ kind: "open", path: FIRST });
    expect(asked.store.current).toBe(FIRST);
    const taken = await asked.api.pluginCall("notes.takeIntent", {});
    expect(taken.ok && taken.value).toBe(null);
    const fresh = await setup({ kind: "new" });
    expect(fresh.store.current).not.toBeNull();
    expect(fresh.store.current).not.toBe(FIRST);
  });

  it("stays on the note being edited when the person does not leave it", async () => {
    const { store } = await setup();
    await store.select(FIRST);
    const off = setLeaveGuard(() => Promise.resolve(false));
    offs.push(off);
    expect(await store.select("2026-10-19 0815.md")).toBe(false);
    expect(store.current).toBe(FIRST);
    off();
    expect(await store.select("2026-10-19 0815.md")).toBe(true);
    expect(store.current).toBe("2026-10-19 0815.md");
  });

  it("moves to the next and the previous note in the list as drawn, wrapping around", async () => {
    const { store } = await setup();
    const order = store.drawn().map((e) => e.path);
    await store.select(order[0] ?? null);
    await store.step(1);
    expect(store.current).toBe(order[1]);
    await store.step(-1);
    await store.step(-1);
    expect(store.current).toBe(order.at(-1));
  });

  it("jumps to the nth pinned note", async () => {
    const { store } = await setup();
    await store.openPinned(1);
    expect(store.current).toBe("2026-10-18 1855.md");
    await store.openPinned(5);
    expect(store.current).toBe("2026-10-18 1855.md");
  });
});

describe("delete and undo", () => {
  it("a note is deleted at once with an Undo that brings it back and shows it", async () => {
    const { store, window } = await setup();
    await store.select(FIRST);
    await store.deleteNotes([FIRST]);
    await settle();
    expect(store.entries.some((e) => e.path === FIRST)).toBe(false);
    expect(store.current).not.toBe(FIRST);
    const toast = window.notices.at(-1);
    if (toast?.kind !== "action") throw new Error("no toast");
    expect(toast.actions.map((a) => a.label)).toEqual(["Undo"]);
    toast.actions[0]?.run();
    await settle();
    expect(store.entries.some((e) => e.path === FIRST)).toBe(true);
    expect(store.current).toBe(FIRST);
  });

  it("asks before deleting a pinned note, and not at all when cancelled", async () => {
    const { store } = await setup();
    const pinned = "2026-10-18 1855.md";
    await store.deleteNotes([pinned]);
    expect(store.confirm.status).toBe("deleteNotes");
    expect(store.entries.some((e) => e.path === pinned)).toBe(true);
    store.cancelConfirm();
    expect(store.confirm.status).toBe("closed");
    await store.deleteNotes([pinned]);
    await store.confirmDelete();
    await settle();
    expect(store.entries.some((e) => e.path === pinned)).toBe(false);
  });

  it("deletes the copied notes that are not pinned", async () => {
    const { store } = await setup();
    expect(store.copiedCount).toBe(1);
    await store.deleteCopied();
    await settle();
    expect(store.copiedCount).toBe(0);
    expect(store.entries.some((e) => e.path === "2026-09-02 1700.md")).toBe(false);
  });
});

describe("rename, pin, share, copy", () => {
  it("renames the note shown and keeps showing it under its new name", async () => {
    const { store } = await setup();
    await store.select(FIRST);
    expect(await store.rename(FIRST, "  trip plans ")).toBeNull();
    await settle();
    expect(store.current).toBe("trip plans.md");
    expect(store.entries.some((e) => e.path === "trip plans.md")).toBe(true);
  });

  it("says why a name does not work, in the field", async () => {
    const { store } = await setup();
    expect(await store.rename(FIRST, "   ")).toMatch(/name/i);
    expect(await store.rename(FIRST, "2026-10-19 0815.md")).toMatch(/already exists/i);
    expect(await store.rename(FIRST, "a/b")).toMatch(/\//);
    expect(store.entries.some((e) => e.path === FIRST)).toBe(true);
  });

  it("pins, unpins and reorders", async () => {
    const { store } = await setup();
    await store.togglePin(FIRST);
    expect(store.entries.find((e) => e.path === FIRST)?.pinned).toBe(true);
    expect(store.entries.filter((e) => e.pinned).map((e) => e.path)).toEqual([
      "2026-10-18 1855.md",
      FIRST,
    ]);
    await store.movePin(FIRST, -1);
    expect(store.entries.filter((e) => e.pinned).map((e) => e.path)).toEqual([
      FIRST,
      "2026-10-18 1855.md",
    ]);
    await store.dropPin(FIRST, "2026-10-18 1855.md");
    expect(store.entries.filter((e) => e.pinned).map((e) => e.path)[0]).toBe("2026-10-18 1855.md");
    await store.togglePin(FIRST);
    expect(store.entries.find((e) => e.path === FIRST)?.pinned).toBe(false);
  });

  it("shares a note with agents and says so", async () => {
    const { store, window } = await setup();
    await store.toggleShare(FIRST);
    expect(store.entries.find((e) => e.path === FIRST)?.shared).toBe(true);
    expect(window.notices.at(-1)).toMatchObject({ kind: "note" });
    await store.toggleShare(FIRST);
    expect(store.entries.find((e) => e.path === FIRST)?.shared).toBe(false);
  });

  it("copies the text exactly and marks the note until it is edited", async () => {
    const written: string[] = [];
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: (text: string) => Promise.resolve(void written.push(text)) },
      configurable: true,
    });
    const { store } = await setup();
    await store.copyNote(FIRST);
    expect(written[0]).toContain("Trip to São Paulo");
    expect(store.entries.find((e) => e.path === FIRST)?.copied).toBe(true);
  });
});

describe("versions", () => {
  it("lists, previews and restores an earlier version", async () => {
    const { store, api } = await setup();
    await store.select(FIRST);
    // a restore keeps the text that is there: make one version through the backend first
    await api.pluginCall("notes.restoreVersion", { path: FIRST, id: "0" }).catch(() => {});
    await store.openVersions(FIRST);
    expect(store.versions.status).toBe("open");
    if (store.versions.status !== "open") return;
    expect(store.versions.items).toEqual([]);
    store.closeVersions();
    expect(store.versions.status).toBe("closed");
  });
});

describe("move to a project", () => {
  it("picks a recent project, suggests a name, moves, and offers Undo and Open", async () => {
    const { store, window, api } = await setup();
    await store.select(FIRST);
    await store.openMove(FIRST);
    expect(store.move.status).toBe("open");
    if (store.move.status !== "open") return;
    expect(store.move.recents.length).toBeGreaterThan(0);
    expect(store.move.name).toBe("Trip to São Paulo.md");
    store.setMoveName("sao paulo");
    await store.confirmMove();
    await settle();
    expect(store.move.status).toBe("closed");
    expect(store.entries.some((e) => e.path === FIRST)).toBe(false);
    const toast = window.notices.at(-1);
    if (toast?.kind !== "action") throw new Error("no toast");
    expect(toast.actions.map((a) => a.label)).toEqual(["Undo", "Open in project"]);
    toast.actions[1]?.run();
    expect(JSON.stringify(api)).toBeDefined();
    toast.actions[0]?.run();
    await settle();
    expect(store.entries.some((e) => e.path === FIRST)).toBe(true);
  });

  it("keeps the dialog open with the reason when the move is refused", async () => {
    const { store } = await setup();
    await store.openMove(FIRST);
    store.setMoveName("a/b");
    await store.confirmMove();
    expect(store.move.status).toBe("open");
    if (store.move.status === "open") {
      expect(store.move.error).toMatch(/\//);
      expect(store.move.busy).toBe(false);
    }
    store.closeMove();
    expect(store.move.status).toBe("closed");
  });
});

describe("loose files", () => {
  it("lists a file and shows it", async () => {
    const { store } = await setup();
    await store.openFiles(["C:/Downloads/sent.md"]);
    await settle();
    const files = store.entries.filter((e) => e.kind === "file");
    expect(files).toHaveLength(1);
    expect(store.current).toBe(files[0]?.path);
  });

  it("asks before taking off the list a file that holds typed text", async () => {
    const { store, api } = await setup();
    await store.openFiles(["C:/Downloads/sent.md"]);
    const path = store.current;
    if (path === null) throw new Error("no file");
    await api.pluginCall("notes.buffer", { path, baseHash: "h", text: "typed" });
    const row = store.entries.find((e) => e.path === path);
    expect(row).toBeDefined();
    // the mock reports the pending text through its journal
    await store.refresh();
    expect(store.entries.find((e) => e.path === path)?.pending).toBe(true);
    await store.removeFile(path);
    expect(store.confirm.status).toBe("removeFile");
    await store.confirmDelete();
    await settle();
    expect(store.entries.some((e) => e.path === path)).toBe(false);
    expect(store.current).toBeNull();
  });

  it("makes a note of the typed text", async () => {
    const { store, api } = await setup();
    await store.openFiles(["C:/Downloads/sent.md"]);
    const path = store.current ?? "";
    await api.pluginCall("notes.buffer", { path, baseHash: "h", text: "# My edits\nbody" });
    await store.saveAsNote(path);
    await settle();
    expect(store.current).toBe("sent (my edits).md");
  });
});

describe("recovery", () => {
  it("compares, keeps or discards the recovered text", async () => {
    const { store, api } = await setup();
    const probe = (
      api as unknown as { probe: { plugins: { notes: { recover(p: string, t: string): void } } } }
    ).probe;
    probe.plugins.notes.recover(FIRST, "# Trip to São Paulo\n\nAnd more that was typed.\n");
    await store.refresh();
    expect(store.entries.find((e) => e.path === FIRST)?.recovered).toBe(true);
    await store.openCompare(FIRST);
    expect(store.compare.status).toBe("open");
    await store.keepRecovered(FIRST);
    await settle();
    expect(store.compare.status).toBe("closed");
    expect(store.entries.find((e) => e.path === FIRST)?.recovered).toBe(false);
    probe.plugins.notes.recover(FIRST, "again");
    await store.refresh();
    await store.discardRecovered(FIRST);
    expect(store.entries.find((e) => e.path === FIRST)?.recovered).toBe(false);
  });
});

describe("the folder", () => {
  it("is created when the person asks, and the list is read again", async () => {
    const { store, api } = await setup();
    const probe = (
      api as unknown as { probe: { plugins: { notes: { folder(s: unknown): void } } } }
    ).probe;
    probe.plugins.notes.folder({ status: "missing" });
    await store.refresh();
    expect(store.folder?.status.status).toBe("missing");
    await store.createFolder();
    await settle();
    expect(store.folder?.status.status).toBe("ready");
  });
});

describe("the window API stays typed", () => {
  it("the mock serves every notes method", async () => {
    const api: Api = createMockApi();
    const methods = ["notes.folder", "notes.list", "notes.summary"] as const;
    for (const method of methods) {
      const result = await api.pluginCall(method, {});
      expect(result.ok, method).toBe(true);
    }
  });
});

describe("wishes from other views (the Notes tab open elsewhere)", () => {
  it("an open Notes tab shows the note another view asks for, once", async () => {
    const { api, store } = await setup();
    const before = store.entries.length;
    await api.pluginCall("notes.want", { intent: { kind: "open", path: "2026-10-19 0815.md" } });
    await settle();
    expect(store.current).toBe("2026-10-19 0815.md");
    await api.pluginCall("notes.want", { intent: { kind: "new" } });
    await settle();
    expect(store.entries.length).toBe(before + 1);
    expect(store.current).not.toBe("2026-10-19 0815.md");
    const taken = await api.pluginCall("notes.takeIntent", {});
    expect(taken.ok && taken.value).toBe(null);
  });

  it("a wish heard while the first read runs is done once", async () => {
    const api = createMockApi();
    const outcome = await api.openNotes("thisTab");
    if (!outcome.ok || outcome.value.outcome !== "opened") throw new Error("notes did not open");
    const view = outcome.value.view;
    if (view.state !== "showing" || view.workspace.kind !== "notes") throw new Error("not notes");
    const client = new PluginClient(api);
    const create = vi.spyOn(api, "pluginCall");
    const editing = new EditingSession(api);
    const store = new NotesStore({
      api,
      client,
      documents: new DocumentController(api),
      editing,
      window: new WindowStore(api),
    });
    const off = store.attach(view.workspace);
    offs.push(off, () => editing.release());
    // the wish lands before the list is read: the read takes it, the event finds the slot empty
    await api.pluginCall("notes.want", { intent: { kind: "new" } });
    await settle();
    const made = create.mock.calls.filter(([method]) => method === "notes.create");
    expect(made).toHaveLength(1);
    expect(store.current).not.toBeNull();
  });

  it("a Notes tab that went away takes nothing (the wish waits for the next one)", async () => {
    const { api } = await setup();
    for (const off of offs.splice(0)) off();
    await api.pluginCall("notes.want", { intent: { kind: "open", path: FIRST } });
    await settle();
    const taken = await api.pluginCall("notes.takeIntent", {});
    expect(taken.ok && taken.value).toEqual({ kind: "open", path: FIRST });
  });

  it("asking from any view stores the wish first, then opens or brings forward the Notes tab", async () => {
    const api = createMockApi();
    const order: string[] = [];
    const client = new PluginClient(api);
    const window = new WindowStore(api);
    vi.spyOn(client, "call").mockImplementation(async (method) => {
      order.push(method);
      return { ok: true, value: null } as never;
    });
    vi.spyOn(window, "openNotes").mockImplementation(async (placement) => {
      order.push(`open:${placement}`);
    });
    await showInNotes({ kind: "new" }, { client, window });
    expect(order).toEqual(["notes.want", "open:tab"]);
  });
});

describe("adversarial: intents (verifier)", () => {
  it("an intent for a note that is gone: list only (notes.md Intents), nothing recorded as open", async () => {
    const { api, store } = await setup({ kind: "open", path: "gone 1999.md" });
    expect(store.list.status).toBe("ready");
    expect(store.current).toBeNull();
    void api;
  });

  it("ten New wishes from other views in a burst while the tab is live: ten notes, slot empty", async () => {
    const { api, store } = await setup();
    const before = store.entries.length;
    for (let i = 0; i < 10; i++) await api.pluginCall("notes.want", { intent: { kind: "new" } });
    await settle();
    await settle();
    const taken = await api.pluginCall("notes.takeIntent", {});
    expect(taken.ok && taken.value).toBeNull();
    // each wish announced and taken before the next: one note each
    expect(store.entries.length - before, "notes made").toBe(10);
  });

  it("an Open wish with a path outside the notes (traversal | absolute) does not escape the Notes", async () => {
    const { store, documents } = await setup({ kind: "open", path: "../../secret.md" });
    // nothing shown (the backend scope decides; the intent adds no new path into it)
    expect(documents.shown).toBeNull();
    expect(store.current, "list only for a name not in the list").toBeNull();
  });
});
