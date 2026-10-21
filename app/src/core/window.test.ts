import { describe, expect, it } from "vitest";
import { createMockApi } from "../api/mock";
import { ERROR_KINDS } from "../api/error";
import type { Api, Result, OpenOutcome } from "../api";
import { setLeaveGuard } from "./editing/guard";
import { errorKey } from "./errors";
import en from "./i18n/locales/en";
import { tabs } from "./shell/tabs.svelte";
import { windowTitle } from "./title";
import { ui } from "./shell/ui.svelte";
import { WindowStore } from "./window.svelte";

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
    const store = await started(api);
    await store.newProject();
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("permark-docs");
    expect(store.notices).toEqual([]);
  });

  it("a folder that cannot be initialized says why and nothing opens", async () => {
    const api = createMockApi();
    const store = await started({
      ...api,
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
