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
const { resolveKey } = await import("../core/commands/dispatch");
const { neutralContext } = await import("../core/conditions/context.svelte");

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
      // next | previous suggestion while editing: one pair of commands for whichever plugin's review is open
      // (correction's | translation's), beside the reading view's next | previous change (narrower: never a conflict)
      expect(map.owners.get("F7"), `mac=${mac}`).toEqual([
        "doc.nextChange",
        "local-ai.nextSuggestion",
      ]);
      expect(map.owners.get("Shift+F7"), `mac=${mac}`).toEqual([
        "doc.prevChange",
        "local-ai.prevSuggestion",
      ]);
      // translate where the caret is: Mod+Alt+t never reaches the app on Linux desktops (it opens a terminal)
      expect(map.owners.get("Mod+Alt+r"), `mac=${mac}`).toEqual(["translation.here"]);
      expect(map.owners.get("Mod+Alt+t"), `mac=${mac}`).toBeUndefined();
      // print the shown document; Mod+p stays Quick open
      expect(map.owners.get("Mod+Alt+p"), `mac=${mac}`).toEqual(["export.print"]);
      expect(map.owners.get("Mod+p"), `mac=${mac}`).toEqual(["search.quickOpen"]);
      expect(map.owners.get("Mod+Shift+s")).toEqual(["notes.moveToProject"]);
      // next | previous: Ctrl+Alt+Arrow switches workspaces on several Linux desktops, so each pair has a
      // PageDown | PageUp key first; the arrows stay as the second key
      expect(map.combos.get("tab.next")?.slice(1), `mac=${mac}`).toEqual([
        "Mod+PageDown",
        "Mod+Alt+ArrowRight",
      ]);
      expect(map.combos.get("tab.previous")?.slice(1), `mac=${mac}`).toEqual([
        "Mod+PageUp",
        "Mod+Alt+ArrowLeft",
      ]);
      expect(map.combos.get("notes.next"), `mac=${mac}`).toEqual([
        "Alt+PageDown",
        "Mod+Alt+ArrowDown",
      ]);
      expect(map.combos.get("notes.previous"), `mac=${mac}`).toEqual([
        "Alt+PageUp",
        "Mod+Alt+ArrowUp",
      ]);
      for (const combo of ["Mod+PageDown", "Mod+PageUp", "Alt+PageDown", "Alt+PageUp"]) {
        expect(map.owners.get(combo)?.length, `${combo} mac=${mac}`).toBe(1);
      }
      // in the Notes tab, also while a note is being typed
      const inNotes = { ...neutralContext(), workspace: "notes" as const };
      const commands = registry.of("command");
      const step = (key: string, init: KeyboardEventInit): string | null => {
        const decision = resolveKey(new KeyboardEvent("keydown", { key, code: key, ...init }), {
          mac,
          keymap: map,
          commands,
          context: inNotes,
          typing: true,
          overlay: "none",
          dialog: false,
        });
        return decision.kind === "run" ? decision.command.id : null;
      };
      const mod = mac ? { metaKey: true } : { ctrlKey: true };
      expect(step("PageDown", { altKey: true }), `mac=${mac}`).toBe("notes.next");
      expect(step("PageUp", { altKey: true }), `mac=${mac}`).toBe("notes.previous");
      expect(step("ArrowDown", { ...mod, altKey: true }), `mac=${mac}`).toBe("notes.next");
      expect(step("ArrowUp", { ...mod, altKey: true }), `mac=${mac}`).toBe("notes.previous");
      expect(requiresOf("notes.moveToProject")).toEqual({
        needs: [{ need: "workspace", kinds: ["notes"] }],
        prefers: [],
      });
    }
  });
});
