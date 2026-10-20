// @vitest-environment jsdom
// Adversarial: the default keymap with the core and every built-in plugin on, Windows | Linux and macOS spelling.
import "../core/testing/media";
import { describe, expect, it, vi } from "vitest";

vi.mock("../api", async () => {
  const { createMockApi } = await import("../api/mock");
  return { api: createMockApi() };
});

const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../core/plugins/registry.svelte");
const { registerCoreCommands } = await import("../core/commands/core");
const { buildKeymap, conflicts } = await import("../core/commands/keymap");

describe("every key with every plugin on", () => {
  it("the new combos have one owner each; the only conflicts are the known Notes-vs-project ones", async () => {
    const registry = new ContributionRegistry();
    registerCoreCommands(registry);
    const host = new PluginHost(registry, pluginLoaders());
    for (const id of Object.keys(pluginLoaders())) await host.enable(id as never);
    const shortcuts = registry.of("shortcut").map((s) => ({ command: s.command, keys: s.keys }));
    const ids = registry.of("command").map((c) => c.id);
    for (const mac of [false, true]) {
      const map = buildKeymap(shortcuts, {}, ids, mac);
      for (const combo of ["Mod+b", "Mod+Alt+b", "Mod+Shift+d", "Mod+Shift+n"]) {
        expect(map.owners.get(combo)?.length, `${combo} mac=${mac}`).toBe(1);
      }
      const found = conflicts(map).map((c) => `${c.combo}=${c.commands.join("|")}`);
      console.log(`mac=${mac} conflicts:`, found);
      // pre-existing at be65da5 (Notes-only vs tab | project commands; requirement-aware check still to come)
      const known = /^Mod\+([1-9]|n|Shift\+c)=/;
      expect(
        found.filter((c) => !known.test(c)),
        `mac=${mac}`,
      ).toEqual([]);
    }
  });
});
