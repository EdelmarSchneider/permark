import { describe, expect, it } from "vitest";
import { createMockApi } from "../api/mock";
import { ERROR_KINDS } from "../api/error";
import type { Api, Result, OpenOutcome } from "../api";
import { errorKey } from "./errors";
import en from "./i18n/locales/en";
import { windowTitle } from "./title";
import { WindowStore } from "./window.svelte";

async function started(api: Api = createMockApi()): Promise<WindowStore> {
  const store = new WindowStore(api);
  await store.start();
  return store;
}

describe("window open flow", () => {
  it("boots on welcome, opens a folder here, then asks before replacing it", async () => {
    const store = await started();
    expect(store.boot.status).toBe("ready");
    expect(store.label).toBe("main");
    expect(store.view.state).toBe("welcome");
    await store.openFolder();
    expect(store.view.state).toBe("showing");
    await store.open("C:/second project", "ask");
    expect(store.choice).toEqual({ status: "asking", path: "C:/second project" });
    await store.answerChoice(null);
    expect(store.choice.status).toBe("closed");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("permark-docs");
    await store.open("C:/second project", "ask");
    await store.answerChoice("here");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("second project");
  });

  it("a second answer without a question does nothing", async () => {
    const store = await started();
    await store.answerChoice("here");
    expect(store.view.state).toBe("welcome");
  });

  it("drop: one item opens, several are refused with a notice", async () => {
    const api = createMockApi();
    let drop: ((e: Parameters<Parameters<Api["onDrop"]>[0]>[0]) => void) | null = null;
    const store = await started({
      ...api,
      onDrop: (handler) => {
        drop = handler;
        return Promise.resolve(() => {});
      },
    });
    drop!({ type: "over", count: 2 });
    expect(store.drag).toEqual({ status: "over", count: 2 });
    drop!({ type: "drop", paths: ["C:/a", "C:/b"] });
    expect(store.drag.status).toBe("idle");
    expect(store.notices.map((n) => n.kind)).toEqual(["dropOne"]);
    drop!({ type: "drop", paths: [] });
    expect(store.notices).toHaveLength(2);
    drop!({ type: "drop", paths: ["C:/文档 x"] });
    await Promise.resolve();
    await Promise.resolve();
    expect(store.view.state).toBe("showing");
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
    await store.open("x", "ask");
    expect(store.notices).toHaveLength(0);
    kind = "notFound";
    for (let i = 0; i < 8; i++) await store.open(`x${i}`, "ask");
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

describe("where a folder opens, and what is remembered", () => {
  it("with open_in = this the dialog never shows and the window is reused", async () => {
    const store = await started();
    await store.openFolder();
    await store.changeSetting({ key: "openIn", value: "this" });
    expect(store.openIn).toBe("this");
    await store.open("C:/second project", "ask");
    expect(store.choice.status).toBe("closed");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("second project");
  });

  it("with open_in = new the folder goes to a new window and this one stays as it is", async () => {
    const store = await started();
    await store.openFolder();
    await store.changeSetting({ key: "openIn", value: "new" });
    await store.open("C:/second project", "ask");
    expect(store.choice.status).toBe("closed");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("permark-docs");
    // an explicit placement is never rewritten by the setting
    await store.open("C:/third project", "thisWindow");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("third project");
  });

  it("the remember check keeps the answer as the setting; cancel keeps nothing", async () => {
    const store = await started();
    await store.openFolder();
    await store.open("C:/second project", "ask");
    await store.answerChoice(null, true);
    expect(store.openIn).toBe("ask");
    await store.open("C:/second project", "ask");
    await store.answerChoice("here", true);
    expect(store.openIn).toBe("this");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("second project");
    // the next open does not ask
    await store.open("C:/third project", "ask");
    expect(store.choice.status).toBe("closed");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("third project");
    await store.changeSetting({ key: "openIn", value: "ask" });
    await store.open("C:/fourth", "ask");
    await store.answerChoice("new", true);
    expect(store.openIn).toBe("new");
  });

  it("a choice made without the check leaves the setting alone", async () => {
    const store = await started();
    await store.openFolder();
    await store.open("C:/second project", "ask");
    await store.answerChoice("new");
    expect(store.openIn).toBe("ask");
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
