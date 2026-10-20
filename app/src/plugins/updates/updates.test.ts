// @vitest-environment jsdom
import "../../core/testing/media";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { UpdateState } from "../../api";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { actionOf, chipOf, statusLine } = await import("./model");
const { UpdatesStore } = await import("./store.svelte");
const { windowStore } = await import("../../core/window.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.distribution;

const STATES: UpdateState[] = [
  { state: "idle" },
  { state: "checking", manual: true },
  { state: "upToDate" },
  { state: "available", version: "0.7.0", notes: "Fixes" },
  { state: "downloading", version: "0.7.0", percent: null },
  { state: "downloading", version: "0.7.0", percent: 42 },
  { state: "ready", version: "0.7.0" },
  { state: "error", stage: "check", message: "offline" },
  { state: "error", stage: "download", message: "bad signature" },
  { state: "error", stage: "install", message: "no installer" },
];

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

afterEach(() => {
  probe.setUpdate({ state: "idle" });
  probe.unsaved = [];
  probe.offer = "0.7.0";
});

describe("what the updater shows", () => {
  it("a chip only when something waits for the person", () => {
    const shown = STATES.filter((s) => chipOf(s) !== null).map((s) => s.state);
    expect(shown).toEqual([
      "available",
      "downloading",
      "downloading",
      "ready",
      "error",
      "error",
      "error",
    ]);
    expect(chipOf({ state: "available", version: "0.7.0", notes: null })?.text).toBe(
      "Update 0.7.0",
    );
    expect(chipOf({ state: "downloading", version: "1", percent: 42 })?.text).toBe("Updating 42%");
    expect(chipOf({ state: "ready", version: "1" })?.tone).toBe("ready");
  });

  it("a sentence for every state, and the action that moves it on", () => {
    for (const state of STATES) expect(statusLine(state), state.state).not.toBe("");
    expect(statusLine({ state: "error", stage: "download", message: "bad signature" })).toContain(
      "bad signature",
    );
    expect(STATES.map(actionOf)).toEqual([
      "check",
      null,
      "check",
      "download",
      null,
      null,
      "restart",
      "check",
      "check",
      "check",
    ]);
  });
});

describe("the updates store", () => {
  it("follows the backend's state and runs the person's actions", async () => {
    const store = new UpdatesStore();
    const stop = store.start();
    await until(() => store.state.state === "idle", "read");
    await store.run("check");
    await until(() => store.state.state === "available", "found");
    await store.run("download");
    await until(() => store.state.state === "ready", "downloaded");
    await store.run("restart");
    expect(probe.restarts).toBe(1);
    stop();
    // after the teardown, announcements no longer reach it
    probe.setUpdate({ state: "upToDate" });
    expect(store.state.state).toBe("ready");
  });

  it("a restart with unsaved edits is refused and says which", async () => {
    const store = new UpdatesStore();
    const stop = store.start();
    probe.setUpdate({ state: "ready", version: "0.7.0" });
    probe.unsaved = ["docs/a.md", "b.md"];
    const before = probe.restarts;
    await store.run("restart");
    expect(probe.restarts).toBe(before);
    expect(windowStore.notices.at(-1)).toMatchObject({
      kind: "note",
      text: "Save your changes first: docs/a.md, b.md",
    });
    stop();
  });

  it("the palette's check tells its answer, even with nothing new", async () => {
    probe.offer = null;
    const store = new UpdatesStore();
    const stop = store.start();
    const before = windowStore.notices.length;
    await store.checkAndTell();
    await until(() => windowStore.notices.length > before, "told");
    expect(windowStore.notices.at(-1)).toMatchObject({ text: "PerMark is up to date." });
    stop();
  });

  it("Later keeps an offer and puts a failure away", async () => {
    const store = new UpdatesStore();
    const stop = store.start();
    probe.setUpdate({ state: "available", version: "0.7.0", notes: null });
    store.popover = true;
    store.later();
    expect(store.popover).toBe(false);
    expect(store.state.state).toBe("available");
    probe.setUpdate({ state: "error", stage: "check", message: "x" });
    store.later();
    await until(() => store.state.state === "idle", "put away");
    stop();
  });

  it("a storm of state announcements ends on the last one", async () => {
    const store = new UpdatesStore();
    const stop = store.start();
    for (let i = 0; i <= 100; i++) {
      probe.setUpdate({ state: "downloading", version: "0.7.0", percent: i });
    }
    expect(store.state).toEqual({ state: "downloading", version: "0.7.0", percent: 100 });
    stop();
  });
});

describe("the distribution plugins", () => {
  it("each adds its parts when on and leaves nothing when off", async () => {
    const loaders = pluginLoaders();
    const parts: Record<string, string[]> = {};
    for (const id of ["updates", "shell-integration"] as PluginId[]) {
      const registry = new ContributionRegistry();
      const host = new PluginHost(registry, loaders);
      await host.enable(id);
      expect(host.state(id), id).toBe("on");
      parts[id] = [...new Set(registry.ownedBy(id).map((c) => c.kind))].sort();
      expect(host.disable(id), id).toBe(0);
      expect(registry.items, id).toHaveLength(0);
    }
    expect(parts).toEqual({
      updates: ["command", "settingsSection", "statusItem"],
      "shell-integration": ["settingsSection"],
    });
  });
});
