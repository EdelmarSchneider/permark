// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { silence } from "../../core/silence/silence.svelte";
import { QuietStore } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

async function setup() {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  const store = new QuietStore(new PluginClient(api, () => MOCK_FOLDER), () => MOCK_FOLDER);
  const stop = store.start();
  await settle();
  return { api, store, stop };
}

afterEach(() => silence.clear());

describe("the quiet store", () => {
  it("silences a folder: what is inside is quiet, the rest is not", async () => {
    const { store, stop } = await setup();
    await store.set(["notes"], "quiet");
    expect(silence.isSilenced("notes")).toBe(true);
    expect(silence.isSilenced("notes/ideas.md")).toBe(true);
    expect(silence.isSilenced("docs/guide.md")).toBe(false);
    expect(store.badge("notes/ideas.md")).toEqual({
      text: "",
      dim: true,
      tip: "Silenced",
    });
    expect(store.badge("docs/guide.md")).toBeNull();
    stop();
  });

  it("listens again, and tells what woke up", async () => {
    const { store, stop } = await setup();
    const woke: string[][] = [];
    const off = silence.onWake((paths) => woke.push([...paths]));
    await store.set(["notes", "docs"], "quiet");
    await store.set(["docs"], "speaking");
    expect(silence.isSilenced("docs/guide.md")).toBe(false);
    expect(silence.isSilenced("notes/ideas.md")).toBe(true);
    expect(woke).toEqual([["docs"]]);
    off();
    stop();
  });

  it("keeps a path that speaks inside a silenced folder", async () => {
    const { store, stop } = await setup();
    await store.set(["docs"], "quiet");
    await store.set(["docs/guide.md"], "speaking");
    expect(silence.isSilenced("docs/other.md")).toBe(true);
    expect(silence.isSilenced("docs/guide.md")).toBe(false);
    stop();
  });

  it("reads the marks of the project when it starts and follows the backend", async () => {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisTab");
    await api.pluginCall("quiet.set", { paths: ["notes"], interest: "quiet" });
    const store = new QuietStore(new PluginClient(api, () => MOCK_FOLDER), () => MOCK_FOLDER);
    const stop = store.start();
    await settle();
    expect(silence.isSilenced("notes/ideas.md")).toBe(true);
    await api.pluginCall("quiet.set", { paths: ["notes"], interest: "speaking" });
    await settle();
    expect(silence.isSilenced("notes/ideas.md")).toBe(false);
    stop();
  });

  it("leaves nothing silenced when the plugin goes", async () => {
    const { store, stop } = await setup();
    await store.set(["notes"], "quiet");
    stop();
    expect(silence.isSilenced("notes/ideas.md")).toBe(false);
  });
});
