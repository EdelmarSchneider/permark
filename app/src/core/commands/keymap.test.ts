import { describe, expect, it } from "vitest";
import { buildKeymap, comboOf, commandsFor, conflicts, heldBy } from "./keymap";

const DEFAULTS = [
  { command: "a", keys: "Mod+p" },
  { command: "b", keys: "Mod+Shift+p" },
  { command: "b", keys: "Mod+Shift+k" },
  { command: "c", keys: "F7" },
];
const COMMANDS = ["a", "b", "c", "d"];

describe("the keyboard map", () => {
  it("holds the defaults of each command, in order", () => {
    const map = buildKeymap(DEFAULTS, {}, COMMANDS);
    expect(map.combos.get("b")).toEqual(["Mod+Shift+p", "Mod+Shift+k"]);
    expect(comboOf(map, "b")).toBe("Mod+Shift+p");
    expect(comboOf(map, "d")).toBeNull();
    expect(commandsFor(map, "Mod+p")).toEqual(["a"]);
  });

  it("lets the person's combo replace the defaults, unbind with an empty one, and ignore a bad one", () => {
    const map = buildKeymap(
      DEFAULTS,
      { a: "Mod+Alt+x", b: "", c: "not a combo", d: "Shift+Mod+q" },
      COMMANDS,
    );
    expect(map.combos.get("a")).toEqual(["Mod+Alt+x"]);
    expect(map.combos.has("b")).toBe(false);
    expect(map.combos.get("c")).toEqual(["F7"]);
    expect(map.combos.get("d")).toEqual(["Mod+Shift+q"]);
    expect(commandsFor(map, "Mod+Shift+p")).toEqual([]);
  });

  it("normalizes spellings, drops repeats and defaults that are no combo", () => {
    const map = buildKeymap(
      [
        { command: "a", keys: "Shift+Mod+p" },
        { command: "a", keys: "Mod+Shift+p" },
        { command: "a", keys: "junk" },
      ],
      {},
      ["a"],
    );
    expect(map.combos.get("a")).toEqual(["Mod+Shift+p"]);
  });

  it("finds the combos two commands hold", () => {
    const map = buildKeymap(DEFAULTS, { c: "Mod+p" }, COMMANDS);
    expect(conflicts(map)).toEqual([{ combo: "Mod+p", commands: ["a", "c"] }]);
    expect(heldBy(map, "Mod+p", "a")).toEqual(["c"]);
    expect(commandsFor(map, "Mod+p")).toEqual(["a", "c"]);
    expect(conflicts(buildKeymap(DEFAULTS, {}, COMMANDS))).toEqual([]);
  });

  it("knows overrides for commands nobody registered (a plugin that is off)", () => {
    const map = buildKeymap([], { "plugin.cmd": "Mod+j" }, []);
    expect(commandsFor(map, "Mod+j")).toEqual(["plugin.cmd"]);
  });
});
