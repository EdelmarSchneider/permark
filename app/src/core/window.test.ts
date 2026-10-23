import { describe, expect, it, vi } from "vitest";
import { createMockApi } from "../api/mock";
import { ERROR_KINDS } from "../api/error";
import type { Api, Result, OpenOutcome } from "../api";
import { setLeaveGuard } from "./editing/guard";
import { errorKey } from "./errors";
import en from "./i18n/locales/en";
import { tabs } from "./shell/tabs.svelte";
import { windowTitle } from "./title";
import { ui } from "./shell/ui.svelte";
import { onScreen, WindowStore } from "./window.svelte";

async function started(api: Api = createMockApi()): Promise<WindowStore> {
  const store = new WindowStore(api);
  await store.start();
  return store;
}

describe("window open flow", () => {
  it("boots on welcome, opens a folder as a tab, an open one comes forward (one place)", async () => {
    const api = createMockApi();
    const store = await started(api);
    expect(store.boot.status).toBe("ready");
    expect(store.label).toBe("main");
    expect(store.window).toBe("main");
    expect(store.view.state).toBe("welcome");
    await store.openFolder();
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("permark-docs");
    await store.open("C:/second project");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("second project");
    // the first one again: its tab comes forward, no new tab
    await store.open("C:/Users/demo/permark-docs");
    const listed = await api.listWindows();
    expect(listed.ok && listed.value[0]?.tabs.map((t) => t.title)).toEqual([
      "permark-docs",
      "second project",
    ]);
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("permark-docs");
  });

  it("this tab: the unsaved prompt runs first; keeping the edits opens nothing", async () => {
    const store = await started();
    // the Welcome view the first tab adopted hears its tab from the backend (else this tab would skip the prompt)
    expect(store.tab).toBeNull();
    await store.openFolder();
    expect(store.tab).not.toBeNull();
    let asked = 0;
    let answer = false;
    const unguard = setLeaveGuard(() => {
      asked++;
      return Promise.resolve(answer);
    });
    await store.open("C:/second project", "thisTab");
    expect(asked).toBe(1);
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("permark-docs");
    answer = true;
    await store.open("C:/second project", "thisTab");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("second project");
    // a new tab or window never asks: this tab keeps its document
    await store.open("C:/third", "tab");
    expect(asked).toBe(2);
    unguard();
  });

  it("leaving a workspace ends focus mode", async () => {
    const api = createMockApi();
    const store = await started(api);
    const stop = tabs.start("main", api);
    await store.openFolder();
    ui.setFocus("on");
    expect(ui.focus).toBe("on");
    // another project in this tab
    await store.open("C:/second project", "thisTab");
    expect(ui.focus).toBe("off");
    ui.setFocus("on");
    await store.closeWorkspace();
    expect(store.view.state).toBe("welcome");
    expect(ui.focus).toBe("off");
    stop();
  });

  it("superseded opens stay quiet, other failures become notices (max 5)", async () => {
    const api = createMockApi();
    let kind: "superseded" | "notFound" = "superseded";
    const failing: Api = {
      ...api,
      openPath: (): Promise<Result<OpenOutcome>> =>
        Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "open_path",
            error: { kind, subject: "x", detail: null },
          },
        }),
    };
    const store = await started(failing);
    await store.open("x");
    expect(store.notices).toHaveLength(0);
    kind = "notFound";
    for (let i = 0; i < 8; i++) await store.open(`x${i}`);
    expect(store.notices).toHaveLength(5);
    const first = store.notices[0];
    if (first) store.dismiss(first.id);
    expect(store.notices).toHaveLength(4);
  });

  it("outside open failures for this window only become notices", async () => {
    const api = createMockApi();
    const store = await started(api);
    api.emit("open-failed", {
      window: "w-9",
      path: "a",
      error: { kind: "io", subject: "a", detail: "x" },
    });
    api.emit("open-failed", {
      window: "main",
      path: "b",
      error: { kind: "io", subject: "b", detail: "x" },
    });
    expect(store.notices).toHaveLength(1);
  });
});

describe("in-place open: what stays on screen", () => {
  const docs = {
    kind: "project" as const,
    id: 1,
    name: "docs",
    root: "C:/docs",
    stateLocation: "profile" as const,
  };
  const plain = { ...docs, id: 2, name: "plain", root: "C:/plain" };

  it("while another workspace opens, the one shown before stays, its document included", () => {
    const shown = onScreen({ state: "showing", workspace: docs, document: "a.md" }, null);
    expect(shown).toEqual({ workspace: docs, document: "a.md" });
    const opening = onScreen(
      { state: "opening", target: { kind: "project", path: "C:/plain" }, showing: docs },
      shown,
    );
    // the same object: nothing on screen changes, nothing that follows it runs again
    expect(opening).toBe(shown);
    // a second open meanwhile (it supersedes the first): still the same
    expect(onScreen({ state: "opening", target: { kind: "notes" }, showing: docs }, opening)).toBe(
      shown,
    );
    expect(onScreen({ state: "showing", workspace: plain, document: null }, opening)).toEqual({
      workspace: plain,
      document: null,
    });
  });

  it("an opening view of another workspace than the screen's shows it bare; nothing shown -> nothing", () => {
    const before = { workspace: plain, document: "x.md" };
    expect(
      onScreen({ state: "opening", target: { kind: "notes" }, showing: docs }, before),
    ).toEqual({ workspace: docs, document: null });
    expect(onScreen({ state: "opening", target: { kind: "notes" }, showing: null }, before)).toBe(
      null,
    );
    expect(onScreen({ state: "welcome" }, before)).toBe(null);
    expect(onScreen({ state: "closing" }, before)).toBe(null);
  });
});

describe("window title", () => {
  it("names document, project and file", () => {
    expect(windowTitle({ state: "welcome" })).toBe("PerMark");
    const project = {
      kind: "project" as const,
      id: 1,
      name: "docs",
      root: "C:/docs",
      stateLocation: "profile" as const,
    };
    expect(windowTitle({ state: "showing", workspace: project, document: null })).toBe(
      "docs — PerMark",
    );
    expect(windowTitle({ state: "showing", workspace: project, document: "a/b/spec.md" })).toBe(
      "spec.md — docs — PerMark",
    );
    const file = {
      kind: "singleFile" as const,
      id: 2,
      name: "x.md",
      path: "C:/x.md",
      folder: "C:/",
    };
    expect(windowTitle({ state: "showing", workspace: file, document: null })).toBe(
      "x.md — PerMark",
    );
    expect(windowTitle({ state: "opening", target: { kind: "notes" }, showing: null })).toBe(
      "PerMark",
    );
  });
});

describe("where a folder opens: the open_in setting", () => {
  it("tab (default): a new tab; this: the active tab; window: a new window, this one as it is", async () => {
    const api = createMockApi();
    const store = await started(api);
    const placements: string[] = [];
    const seen: Api = {
      ...api,
      openPath: (path, placement) => {
        placements.push(placement);
        return api.openPath(path, placement);
      },
    };
    const spy = await started(seen);
    expect(spy.openIn).toBe("tab");
    await spy.open("C:/a");
    await spy.changeSetting({ key: "openIn", value: "this" });
    expect(spy.openIn).toBe("this");
    await spy.open("C:/b");
    await spy.changeSetting({ key: "openIn", value: "window" });
    await spy.open("C:/c");
    expect(spy.view.state === "showing" && spy.view.workspace.name).toBe("b");
    // an explicit placement is never rewritten by the setting
    await spy.open("C:/d", "tab");
    expect(placements).toEqual(["tab", "thisTab", "newWindow", "tab"]);
    expect(store.boot.status).toBe("ready");
  });

  it("the Notes follow the same placement", async () => {
    const api = createMockApi();
    const placements: string[] = [];
    const store = await started({
      ...api,
      openNotes: (placement) => {
        placements.push(placement);
        return api.openNotes(placement);
      },
    });
    await store.openNotes();
    await store.changeSetting({ key: "openIn", value: "window" });
    await store.openNotes();
    expect(placements).toEqual(["tab", "newWindow"]);
  });
});

describe("initializing a folder", () => {
  it("New project picks a folder, initializes it and opens it", async () => {
    const api = createMockApi();
    const inits: string[] = [];
    const store = await started({
      ...api,
      folderEmpty: () => Promise.resolve({ ok: true, value: true }),
      projectInit: (path) => {
        inits.push(path);
        return api.projectInit(path);
      },
    });
    expect(await store.newProject()).toBeNull();
    expect(inits).toEqual(["C:/Users/demo/permark-docs"]);
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("permark-docs");
    expect(store.notices).toEqual([]);
  });

  it("a folder with files in it (or one that cannot be read) comes back to be asked about: nothing is written", async () => {
    const api = createMockApi();
    const inits: string[] = [];
    for (const empty of [
      { ok: true as const, value: false },
      {
        ok: false as const,
        error: { kind: "command" as const, command: "folder_empty" as const, message: "gone" },
      },
    ]) {
      const store = await started({
        ...api,
        folderEmpty: () => Promise.resolve(empty),
        projectInit: (path) => {
          inits.push(path);
          return api.projectInit(path);
        },
      });
      expect(await store.newProject()).toEqual({ ask: "C:/Users/demo/permark-docs" });
      expect(store.view.state).toBe("welcome");
    }
    expect(inits).toEqual([]);
  });

  it("a folder that cannot be initialized says why and nothing opens", async () => {
    const api = createMockApi();
    const store = await started({
      ...api,
      folderEmpty: () => Promise.resolve({ ok: true, value: true }),
      projectInit: () =>
        Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "project_init",
            error: { kind: "denied", subject: "C:/locked", detail: null },
          },
        }),
    });
    await store.newProject();
    expect(store.view.state).toBe("welcome");
    expect(store.notices.map((n) => n.kind)).toEqual(["initFailed"]);
    expect(await store.initProject("C:/locked")).toBe(false);
  });

  it("the window of an initialized project hears the new state location and its plugins start over", async () => {
    const api = createMockApi();
    const store = await started(api);
    await store.openFolder();
    const before = store.view;
    expect(
      before.state === "showing" &&
        before.workspace.kind === "project" &&
        before.workspace.stateLocation,
    ).toBe("profile");
    expect(await store.initProject("C:/Users/demo/permark-docs")).toBe(true);
    const after = store.view;
    expect(
      after.state === "showing" &&
        after.workspace.kind === "project" &&
        after.workspace.stateLocation,
    ).toBe("project");
  });
});

describe("errors in words", () => {
  it("every backend error kind has a message key", () => {
    for (const kind of ERROR_KINDS) expect(Object.keys(en)).toContain(errorKey(kind));
  });
});

describe("timed notices", () => {
  it("close by themselves; pointer or focus on one pauses its clock, leaving resumes it (F2)", () => {
    vi.useFakeTimers();
    try {
      const store = new WindowStore(createMockApi());
      const gone: string[] = [];
      store.notifyTimed({
        text: "3 projects included.",
        actions: [],
        closeAfterMs: 8000,
        onGone: (how) => gone.push(how),
      });
      const id = store.notices[0]!.id;
      vi.advanceTimersByTime(5000);
      store.holdNotice(id, "pointer");
      store.holdNotice(id, "focus");
      vi.advanceTimersByTime(60_000);
      expect(store.notices).toHaveLength(1);
      // the pointer leaves, the focus is still there: it waits
      store.releaseNotice(id, "pointer");
      vi.advanceTimersByTime(60_000);
      expect(store.notices).toHaveLength(1);
      store.releaseNotice(id, "focus");
      vi.advanceTimersByTime(2999);
      expect(store.notices).toHaveLength(1);
      vi.advanceTimersByTime(1);
      expect(store.notices).toHaveLength(0);
      expect(gone).toEqual(["timeout"]);
    } finally {
      vi.useRealTimers();
    }
  });

  // adversarial: dismissed, button, pushed out by newer ones, held after it went
  it("say how they went, once; a gone notice holds nothing", () => {
    vi.useFakeTimers();
    try {
      const store = new WindowStore(createMockApi());
      const gone: string[] = [];
      for (let n = 0; n < 7; n += 1) {
        store.notifyTimed({
          text: `n${n}`,
          actions: [],
          closeAfterMs: 8000,
          onGone: (how) => gone.push(`${n}:${how}`),
        });
      }
      expect(store.notices).toHaveLength(5);
      expect(gone).toEqual(["0:pushedOut", "1:pushedOut"]);
      const [a, b] = store.notices;
      store.dismiss(a!.id);
      store.dismiss(b!.id, "action");
      store.dismiss(b!.id, "action");
      store.holdNotice(a!.id, "pointer");
      vi.advanceTimersByTime(8000);
      expect(gone).toEqual([
        "0:pushedOut",
        "1:pushedOut",
        "2:dismissed",
        "3:action",
        "4:timeout",
        "5:timeout",
        "6:timeout",
      ]);
      expect(store.notices).toHaveLength(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
