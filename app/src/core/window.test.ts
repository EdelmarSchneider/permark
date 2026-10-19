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

describe("errors in words", () => {
  it("every backend error kind has a message key", () => {
    for (const kind of ERROR_KINDS) expect(Object.keys(en)).toContain(errorKey(kind));
  });
});
