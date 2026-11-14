// @vitest-environment jsdom
// Favorite writes of the symbols panel: one settings write per change, said in the window's status line; a refusal
// is said too and nothing else changes (the value comes back only through `settings-changed`). Quick changes are
// written one after the other, each from the value the one before it wrote.
import "../../core/testing/media";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApiError, SettingChange, SymbolFavorites } from "../../api";
import { arrangement } from "../../core/shell/arrangement.svelte";
import { windowStore } from "../../core/window.svelte";
import { symbols } from "./store.svelte";

function favorites(chars: string[], groups: string[] = []): SymbolFavorites {
  return { v: 1, chars, groups };
}

/** What `settings-changed` does to the window: a new read of the settings. */
function news(value: SymbolFavorites | null): void {
  windowStore.settingsReads += 1;
  windowStore.symbolFavorites = value;
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
  symbols.reset();
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

    news(favorites(["α"], ["greek"]));
    await symbols.toggleChar({ char: "α", names: ["alpha"] });
    expect(writes.at(-1)).toEqual({ key: "symbolFavorites", value: favorites([], ["greek"]) });
    expect(said).toHaveBeenLastCalledWith("Removed alpha from favorites");
  });

  it("removing the last favorite removes the key", async () => {
    const { writes } = spies();
    news(favorites(["α"]));
    await symbols.toggleChar({ char: "α", names: [] });
    expect(writes).toEqual([{ key: "symbolFavorites", value: null }]);
  });

  it("pins and unpins a group by its name, said with its label", async () => {
    const { writes, said } = spies();
    await symbols.toggleGroup("greek");
    expect(writes).toEqual([{ key: "symbolFavorites", value: favorites([], ["greek"]) }]);
    expect(said).toHaveBeenLastCalledWith("Added the group Greek to favorites");
    news(favorites([], ["greek"]));
    await symbols.toggleGroup("greek");
    expect(writes.at(-1)).toEqual({ key: "symbolFavorites", value: null });
    expect(said).toHaveBeenLastCalledWith("Removed the group Greek from favorites");
  });

  it("two quick toggles keep both, before the settings news of the first arrives", async () => {
    const { writes } = spies();
    await Promise.all([
      symbols.toggleChar({ char: "α", names: ["alpha"] }),
      symbols.toggleGroup("greek"),
    ]);
    expect(writes).toEqual([
      { key: "symbolFavorites", value: favorites(["α"]) },
      { key: "symbolFavorites", value: favorites(["α"], ["greek"]) },
    ]);
    // once news landed, the next step starts from it (another window may have changed it too)
    news(favorites(["α", "β"], ["greek"]));
    await symbols.toggleChar({ char: "α", names: ["alpha"] });
    expect(writes.at(-1)).toEqual({
      key: "symbolFavorites",
      value: favorites(["β"], ["greek"]),
    });
  });

  it("news that arrives while a write is on its way wins over the value written", async () => {
    const writes: SettingChange[] = [];
    vi.spyOn(windowStore, "trySetting").mockImplementation((change) => {
      writes.push(change);
      // the backend sends `settings-changed` before it answers
      news(change.key === "symbolFavorites" ? change.value : null);
      return Promise.resolve(null);
    });
    vi.spyOn(arrangement, "say").mockImplementation(() => {});
    await symbols.toggleChar({ char: "α", names: [] });
    // another window cleared every favorite
    news(null);
    await symbols.toggleChar({ char: "β", names: [] });
    expect(writes.at(-1)).toEqual({ key: "symbolFavorites", value: favorites(["β"]) });
  });

  it("a write waits for the one before it", async () => {
    const writes: SettingChange[] = [];
    const gate: { release: (() => void) | null } = { release: null };
    vi.spyOn(windowStore, "trySetting").mockImplementation((change) => {
      writes.push(change);
      if (writes.length > 1) return Promise.resolve(null);
      return new Promise((resolve) => (gate.release = () => resolve(null)));
    });
    vi.spyOn(arrangement, "say").mockImplementation(() => {});
    const first = symbols.toggleChar({ char: "α", names: [] });
    const second = symbols.toggleChar({ char: "β", names: [] });
    await Promise.resolve();
    await Promise.resolve();
    expect(writes).toHaveLength(1);
    gate.release?.();
    await Promise.all([first, second]);
    expect(writes.at(-1)).toEqual({ key: "symbolFavorites", value: favorites(["α", "β"]) });
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
