// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Api, PluginMethod, PluginMethods, Result } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { Box } from "../../core/testing/box.svelte";
import { ReviewStore } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 6; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

type Shown = { path: string | null; hash: string | null; markdown: boolean };
const GUIDE = "docs/guide.md";

async function setup(options: { conflictOn?: PluginMethod } = {}) {
  const mock = createMockApi();
  await mock.openPath(MOCK_FOLDER, "thisWindow");
  const hash = await mock.pluginCall("review.open", { path: GUIDE });
  const guideHash = hash.ok ? hash.value.hash : "";
  const api: Api = {
    ...mock,
    pluginCall: async <M extends PluginMethod>(method: M, args: PluginMethods[M]["args"]) => {
      if (method === options.conflictOn) {
        const failure: Result<PluginMethods[M]["result"]> = {
          ok: false,
          error: {
            kind: "backend",
            command: "plugin_call",
            error: { kind: "conflict", subject: GUIDE, detail: "stale" },
          },
        };
        return failure;
      }
      return mock.pluginCall(method, args);
    },
  };
  const shown = new Box<Shown>({ path: GUIDE, hash: guideHash, markdown: true });
  const notes: string[] = [];
  let repaints = 0;
  const store = new ReviewStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    (text) => notes.push(text),
    {
      shown: () => shown.value,
      repaint: () => {
        repaints += 1;
      },
    },
  );
  const stop = store.start();
  await settle();
  return { mock, store, stop, shown, notes, guideHash, repaints: () => repaints };
}

afterEach(() => vi.restoreAllMocks());

describe("the review store", () => {
  it("offers the changes of a document that has some, and nothing for one that has none", async () => {
    const { store, shown, stop } = await setup();
    expect(store.state).toMatchObject({ status: "offer", path: GUIDE, count: 2 });
    shown.value = { path: "README.md", hash: "ff".repeat(8), markdown: true };
    await settle();
    expect(store.state.status).toBe("idle");
    stop();
  });

  it("shows the changes, keeps one, takes it back, reverts the rest and ends", async () => {
    const { store, notes, stop } = await setup();
    await store.begin();
    expect(store.state).toMatchObject({ status: "reviewing", at: 0 });
    expect(store.reviewing?.count).toBe(2);
    await store.keep();
    expect(store.reviewing?.count).toBe(1);
    expect(store.undoable).toBe(1);
    await store.undo();
    expect(store.reviewing?.count).toBe(2);
    expect(store.undoable).toBe(0);
    await store.revertAll();
    expect(store.state.status).toBe("idle");
    expect(notes.some((n) => n.length > 0)).toBe(true);
    stop();
  });

  it("moves between the changes, wrapping, and says where it is", async () => {
    const { store, stop } = await setup();
    await store.begin();
    store.go(1);
    expect(store.current).toBe(1);
    store.go(1);
    expect(store.current).toBe(0);
    store.go(-1);
    expect(store.current).toBe(1);
    expect(store.announcement).toMatch(/2/);
    stop();
  });

  it("repaints the page when the review starts, moves, and ends", async () => {
    const { store, repaints, stop } = await setup();
    const before = repaints();
    await store.begin();
    expect(repaints()).toBeGreaterThan(before);
    const mid = repaints();
    store.go(1);
    expect(repaints()).toBeGreaterThan(mid);
    const end = repaints();
    await store.done();
    expect(repaints()).toBeGreaterThan(end);
    expect(store.state.status).toBe("idle");
    stop();
  });

  it("lets go of a document when the person leaves it", async () => {
    const { mock, store, shown, stop } = await setup();
    await store.begin();
    shown.value = { path: "README.md", hash: "ee".repeat(8), markdown: true };
    await settle();
    expect(mock.probe.plugins.calls).toContain("review.release");
    expect(store.state.status).toBe("idle");
    stop();
  });

  it("looks again when the file changed under the review, keeping the place", async () => {
    const { store, shown, stop } = await setup();
    await store.begin();
    store.go(1);
    shown.value = { path: GUIDE, hash: "dd".repeat(8), markdown: true };
    await settle();
    expect(store.state).toMatchObject({ status: "reviewing", at: 1 });
    expect(store.announcement).not.toBe("");
    stop();
  });

  it("a stale answer refreshes the review and says so; nothing else changes", async () => {
    const { store, notes, stop } = await setup({ conflictOn: "review.revert" });
    await store.begin();
    await store.revert(0);
    await settle();
    expect(notes.at(0)).toBeDefined();
    expect(store.reviewing?.count).toBe(2);
    expect(store.undoable).toBe(0);
    stop();
  });

  it("will not review what is not a Markdown document, and says why when asked", async () => {
    const { store, shown, notes, stop } = await setup();
    shown.value = { path: "notes.txt", hash: "aa".repeat(8), markdown: false };
    await settle();
    expect(store.state.status).toBe("idle");
    await store.begin();
    expect(store.reviewing).toBeNull();
    expect(notes).toHaveLength(0);
    stop();
  });

  it("the basis can be changed and the review is drawn again", async () => {
    const { store, mock, stop } = await setup();
    await store.begin();
    await store.setBasis("commit");
    expect(store.basis).toBe("commit");
    expect(mock.probe.plugins.calls).toContain("review.setBasis");
    expect(store.reviewing?.canKeep).toBe(false);
    stop();
  });

  it("teardown leaves it idle", async () => {
    const { store, stop } = await setup();
    await store.begin();
    stop();
    expect(store.state.status).toBe("idle");
  });
});
