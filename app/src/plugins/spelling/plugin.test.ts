// @vitest-environment jsdom
// The spelling plugin on and off: what it adds (the status item that hosts its menus, its settings, the step to the
// next misspelling with no default key) and that switching it off leaves nothing.
import "../../core/testing/media";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const actual = await vi.importActual<typeof import("../../api")>("../../api");
  const { createMockApi } = await import("../../api/mock");
  return { ...actual, api: createMockApi() };
});

const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { editing } = await import("../../core/editing/session.svelte");

describe("the spelling plugin", () => {
  it("adds its status item, settings and command; off, nothing is left and the webview checks as before", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("spelling");
    expect(host.state("spelling")).toBe("on");
    const owned = registry.ownedBy("spelling");
    expect([...new Set(owned.map((c) => c.kind))].sort()).toEqual([
      "command",
      "settingsSection",
      "statusItem",
    ]);
    expect(registry.of("shortcut")).toEqual([]);
    const next = registry.of("command").find((c) => c.id === "spelling.next");
    expect(next?.label).toBe("spelling.next.command");
    // nothing edited: nothing to step to, and the webview keeps its own check (no dictionary is ready for it)
    expect(next?.when?.()).toBe(false);
    expect(editing.nativeSpellcheck).toBe(true);
    expect(host.disable("spelling"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    expect(editing.nativeSpellcheck).toBe(true);
  });
});
