// @vitest-environment jsdom
// Favorite writes of the symbols panel: one settings write per change, said in the window's status line; a refusal
// is said too and nothing else changes (the value comes back only through `settings-changed`).
import "../../core/testing/media";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApiError, SettingChange, SymbolFavorites } from "../../api";
import { arrangement } from "../../core/shell/arrangement.svelte";
import { windowStore } from "../../core/window.svelte";
import { symbols } from "./store.svelte";

function favorites(chars: string[], groups: string[] = []): SymbolFavorites {
  return { v: 1, chars, groups };
}

function spies(refusal: ApiError | null = null) {
  const writes: SettingChange[] = [];
  vi.spyOn(windowStore, "trySetting").mockImplementation((change) => {
    writes.push(change);
    return Promise.resolve(refusal);
  });
  const said = vi.spyOn(arrangement, "say").mockImplementation(() => {});
  return { writes, said };
}

afterEach(() => {
  vi.restoreAllMocks();
  windowStore.symbolFavorites = null;
});

describe("favorite writes", () => {
  it("adds a character, then removes it, and says each", async () => {
    const { writes, said } = spies();
    await symbols.toggleChar({ char: "α", names: ["alpha"] });
    expect(writes).toEqual([{ key: "symbolFavorites", value: favorites(["α"]) }]);
    expect(said).toHaveBeenLastCalledWith("Added alpha to favorites");
    // the local value is never changed: the new one arrives from the settings
    expect(windowStore.symbolFavorites).toBeNull();

    windowStore.symbolFavorites = favorites(["α"], ["greek"]);
    await symbols.toggleChar({ char: "α", names: ["alpha"] });
    expect(writes.at(-1)).toEqual({ key: "symbolFavorites", value: favorites([], ["greek"]) });
    expect(said).toHaveBeenLastCalledWith("Removed alpha from favorites");
  });

  it("removing the last favorite removes the key", async () => {
    const { writes } = spies();
    windowStore.symbolFavorites = favorites(["α"]);
    await symbols.toggleChar({ char: "α", names: [] });
    expect(writes).toEqual([{ key: "symbolFavorites", value: null }]);
  });

  it("pins and unpins a group by its name, said with its label", async () => {
    const { writes, said } = spies();
    await symbols.toggleGroup("greek");
    expect(writes).toEqual([{ key: "symbolFavorites", value: favorites([], ["greek"]) }]);
    expect(said).toHaveBeenLastCalledWith("Added the group Greek to favorites");
    windowStore.symbolFavorites = favorites([], ["greek"]);
    await symbols.toggleGroup("greek");
    expect(writes.at(-1)).toEqual({ key: "symbolFavorites", value: null });
    expect(said).toHaveBeenLastCalledWith("Removed the group Greek from favorites");
  });

  it("a refused write is said politely", async () => {
    const { said } = spies({
      kind: "backend",
      command: "settings_set",
      error: { kind: "invalidData", subject: "symbolFavorites", detail: "symbolFavorites" },
    });
    await symbols.toggleChar({ char: "α", names: ["alpha"] });
    expect(said).toHaveBeenLastCalledWith("That cannot be kept as a favorite.");
  });

  it("a change the settings would refuse writes nothing", async () => {
    const { writes, said } = spies();
    await symbols.toggleChar({ char: "→".repeat(17), names: [] });
    expect(writes).toEqual([]);
    expect(said).toHaveBeenLastCalledWith("That cannot be kept as a favorite.");
  });
});
