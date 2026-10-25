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
  it("the new combos have one owner each; no command fights another over a key", async () => {
    const registry = new ContributionRegistry();
    registerCoreCommands(registry);
    const host = new PluginHost(registry, pluginLoaders());
    for (const id of Object.keys(pluginLoaders())) await host.enable(id as never);
    const shortcuts = registry.of("shortcut").map((s) => ({ command: s.command, keys: s.keys }));
    const ids = registry.of("command").map((c) => c.id);
    for (const mac of [false, true]) {
      const map = buildKeymap(shortcuts, {}, ids, mac);
      // Mod+Shift+s: move the open note to a project, in the Notes tab only (UX regressions #13)
      for (const combo of ["Mod+b", "Mod+Alt+b", "Mod+Shift+d", "Mod+Shift+n", "Mod+Shift+s"]) {
        expect(map.owners.get(combo)?.length, `${combo} mac=${mac}`).toBe(1);
      }
      const requiresOf = (id: string) => registry.of("command").find((c) => c.id === id)?.requires;
      // Notes-only commands share Mod+1..9 | Mod+n | Mod+Shift+c with tab | project commands: never a conflict
      // (narrower scope wins in the Notes tab; `search.md` Keyboard)
      for (const combo of ["Mod+1", "Mod+9", "Mod+n", "Mod+Shift+c"]) {
        expect(map.owners.get(combo)?.length, `${combo} mac=${mac}`).toBe(2);
      }
      expect(conflicts(map, requiresOf), `mac=${mac}`).toEqual([]);
      expect(map.owners.get("Mod+Shift+s")).toEqual(["notes.moveToProject"]);
      expect(requiresOf("notes.moveToProject")).toEqual({
        needs: [{ need: "workspace", kinds: ["notes"] }],
        prefers: [],
      });
    }
  });
});
