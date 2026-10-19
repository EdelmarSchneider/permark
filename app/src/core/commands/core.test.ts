// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { ContributionRegistry, CORE_OWNER } = await import("../plugins/registry.svelte");
const { registerCoreCommands } = await import("./core");
const { buildKeymap, conflicts } = await import("./keymap");
const { parseCombo } = await import("./combo");
const { en } = await import("../i18n/table");

describe("the core's commands", () => {
  const registry = new ContributionRegistry();
  const off = registerCoreCommands(registry);
  const commands = registry.of("command");
  const shortcuts = registry.of("shortcut");

  it("are owned by the core, have unique ids and a label in the language table", () => {
    expect(commands.length).toBeGreaterThan(20);
    expect(new Set(commands.map((c) => c.id)).size).toBe(commands.length);
    for (const command of commands) {
      expect(command.owner).toBe(CORE_OWNER);
      expect(command.id).toMatch(/^[a-z]+\.[A-Za-z]+$/);
      expect(Object.hasOwn(en, command.label), command.id).toBe(true);
    }
  });

  it("have default keys that parse, point at a command and do not fight", () => {
    const ids = new Set(commands.map((c) => c.id));
    for (const shortcut of shortcuts) {
      expect(parseCombo(shortcut.keys), shortcut.keys).not.toBeNull();
      expect(ids.has(shortcut.command), shortcut.command).toBe(true);
    }
    const map = buildKeymap(
      shortcuts.map((s) => ({ command: s.command, keys: s.keys })),
      {},
      [...ids],
    );
    expect(conflicts(map)).toEqual([]);
  });

  it("keep the box commands out of the palette and name the keys the app is known by", () => {
    const hidden = commands.filter((c) => c.inPalette === false).map((c) => c.id);
    expect(hidden.sort()).toEqual(["palette.open", "search.everywhere", "search.quickOpen"]);
    const keyOf = (id: string): string | undefined => shortcuts.find((s) => s.command === id)?.keys;
    expect(keyOf("search.quickOpen")).toBe("Mod+p");
    expect(keyOf("search.all")).toBe("Mod+Shift+f");
    expect(keyOf("search.everywhere")).toBe("Mod+t");
    expect(keyOf("palette.open")).toBe("Mod+Shift+p");
  });

  it("leave nothing behind when removed", () => {
    off();
    expect(registry.items).toEqual([]);
  });
});
