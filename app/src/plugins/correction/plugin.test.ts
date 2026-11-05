// @vitest-environment jsdom
// Plugin `correction` switched on | off: its parts (button with the mode switch, review bar, commands + keys) and
// nothing left after; dormant (nothing offered, no key acts) while no model is assigned to job "correction".
import "../../core/testing/media";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { correction } = await import("./store.svelte");
const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

afterEach(() => {
  mock.probe.correction.available = null;
});

async function on() {
  const registry = new ContributionRegistry();
  const host = new PluginHost(registry, pluginLoaders());
  await host.enable("correction");
  return { registry, host };
}

describe("plugin correction", () => {
  it("adds its parts when switched on and leaves nothing when switched off", async () => {
    const { registry, host } = await on();
    expect(host.state("correction")).toBe("on");
    expect([...new Set(registry.ownedBy("correction").map((c) => c.kind))].sort()).toEqual([
      "command",
      "docBar",
      "editAction",
      "shortcut",
    ]);
    expect(
      registry
        .of("shortcut")
        .map((s) => `${s.command} ${s.keys}`)
        .sort(),
    ).toEqual(["correction.correct Mod+Alt+k", "correction.next F7", "correction.prev Shift+F7"]);
    expect(host.disable("correction"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    expect(correction.available).toBe(false);
  });

  it("is dormant until a model is assigned: no button, no command", async () => {
    mock.probe.correction.available = false;
    const { registry, host } = await on();
    await vi.waitFor(() => expect(mock.probe.correction.calls).toContain("status"));
    expect(correction.available).toBe(false);
    for (const item of registry.ownedBy("correction")) {
      if (item.kind === "editAction" || item.kind === "command") expect(item.when?.()).toBe(false);
    }
    host.disable("correction");
  });

  it("asks again when the Local AI assignments change", async () => {
    mock.probe.correction.available = false;
    const { host } = await on();
    await vi.waitFor(() => expect(mock.probe.correction.calls).toContain("status"));
    mock.probe.correction.available = null;
    await mock.pluginCall("local-ai.assign", { task: "correction", model: "gemma-4-e2b-it-q4_0" });
    await vi.waitFor(() => expect(correction.available).toBe(true));
    host.disable("correction");
  });
});
