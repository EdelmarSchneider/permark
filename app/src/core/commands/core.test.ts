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
      expect(command.id).toMatch(/^[a-z]+\.[A-Za-z]+[1-8]?$/);
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
    const requiresOf = (id: string) => commands.find((c) => c.id === id)?.requires;
    expect(conflicts(map, requiresOf)).toEqual([]);
    // the same on macOS (Ctrl+Tab there is the Control key, its own combo)
    const mac = buildKeymap(
      shortcuts.map((s) => ({ command: s.command, keys: s.keys })),
      {},
      [...ids],
      true,
    );
    expect(conflicts(mac, requiresOf)).toEqual([]);
    expect(mac.combos.get("tab.next")).toEqual(["Ctrl+Tab", "Mod+Alt+ArrowRight"]);
    expect(map.combos.get("tab.next")).toEqual(["Mod+Tab", "Mod+Alt+ArrowRight"]);
  });

  it("keep the box commands out of the palette and name the keys the app is known by", () => {
    const hidden = commands.filter((c) => c.inPalette === false).map((c) => c.id);
    expect(hidden.sort()).toEqual([
      "palette.open",
      "search.everywhere",
      "search.quickOpen",
      ...[1, 2, 3, 4, 5, 6, 7, 8].map((n) => `tab.select${n}`),
      "tab.selectLast",
    ]);
    const keyOf = (id: string): string | undefined => shortcuts.find((s) => s.command === id)?.keys;
    expect(keyOf("search.quickOpen")).toBe("Mod+p");
    expect(keyOf("search.all")).toBe("Mod+Shift+f");
    expect(keyOf("search.everywhere")).toBe("Mod+t");
    expect(keyOf("palette.open")).toBe("Mod+Shift+p");
  });

  it("move to the next and previous change with F7 and Shift+F7, and say when there is none", async () => {
    const keyOf = (id: string): string | undefined => shortcuts.find((s) => s.command === id)?.keys;
    expect(keyOf("doc.nextChange")).toBe("F7");
    expect(keyOf("doc.prevChange")).toBe("Shift+F7");
    const next = commands.find((c) => c.id === "doc.nextChange");
    expect(next?.when?.()).toBe(false); // no plugin gives a step
    await next?.run();
    const { windowStore } = await import("../window.svelte");
    expect(windowStore.notices.at(-1)).toMatchObject({ kind: "note", text: en["doc.noChange"] });
  });

  it("switch long lines wrapping with Alt+Z: saved, said, and twice is back", async () => {
    const keyOf = (id: string): string | undefined => shortcuts.find((s) => s.command === id)?.keys;
    expect(keyOf("doc.wrapLines")).toBe("Alt+z");
    const { api } = await import("../../api");
    const { look } = await import("../look.svelte");
    const { windowStore } = await import("../window.svelte");
    const set = vi.spyOn(api, "settingsSet");
    const wrap = commands.find((c) => c.id === "doc.wrapLines");
    look.wrap = true;
    await wrap?.run();
    expect(look.wrap).toBe(false);
    expect(set).toHaveBeenLastCalledWith({ key: "wrapLines", value: false });
    expect(windowStore.notices.at(-1)).toMatchObject({ kind: "note", text: en["wrap.off"] });
    await wrap?.run();
    expect(look.wrap).toBe(true);
    expect(set).toHaveBeenLastCalledWith({ key: "wrapLines", value: true });
    expect(windowStore.notices.at(-1)).toMatchObject({ kind: "note", text: en["wrap.on"] });
    // refused: the views go back, the person is told
    set.mockResolvedValueOnce({
      ok: false,
      error: {
        kind: "backend",
        command: "settings_set",
        error: { kind: "io", subject: "settings.toml", detail: null },
      },
    });
    await wrap?.run();
    expect(look.wrap).toBe(true);
    const told = windowStore.notices.at(-1);
    expect(told?.kind === "note" ? told.text : "").toMatch(/^Couldn't keep this setting/);
    set.mockRestore();
  });

  it("leave nothing behind when removed", () => {
    off();
    expect(registry.items).toEqual([]);
  });
});
