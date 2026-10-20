import { describe, expect, it } from "vitest";
import type { Requirement } from "../conditions/available";
import {
  buildKeymap,
  clash,
  comboOf,
  commandsFor,
  conflicts,
  heldBy,
  narrower,
  scopeOf,
  type RequiresOf,
} from "./keymap";

/** No command has a requirement: every one may run anywhere. */
const ANYWHERE: RequiresOf = () => undefined;

function kinds(...list: ("welcome" | "project" | "singleFile" | "notes")[]): Requirement {
  return { needs: [{ need: "workspace", kinds: list }], prefers: [] };
}

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
    expect(conflicts(map, ANYWHERE)).toEqual([{ combo: "Mod+p", commands: ["a", "c"] }]);
    expect(heldBy(map, "Mod+p", "a", ANYWHERE)).toEqual(["c"]);
    expect(commandsFor(map, "Mod+p")).toEqual(["a", "c"]);
    expect(conflicts(buildKeymap(DEFAULTS, {}, COMMANDS), ANYWHERE)).toEqual([]);
  });

  it("lets two commands share a combo when they never run in the same place, or one is narrower", () => {
    const requires: Record<string, Requirement> = {
      "file.new": kinds("project"),
      "notes.newHere": kinds("notes"),
      "notes.pinned1": kinds("notes"),
      "doc.copyDocument": kinds("project", "singleFile", "notes"),
      "notes.copy": kinds("notes"),
    };
    const requiresOf: RequiresOf = (id) => requires[id];
    const map = buildKeymap(
      [
        { command: "file.new", keys: "Mod+n" },
        { command: "notes.newHere", keys: "Mod+n" },
        { command: "tab.select1", keys: "Mod+1" },
        { command: "notes.pinned1", keys: "Mod+1" },
        { command: "doc.copyDocument", keys: "Mod+Shift+c" },
        { command: "notes.copy", keys: "Mod+Shift+c" },
      ],
      {},
      [
        "file.new",
        "tab.select1",
        "doc.copyDocument",
        "notes.newHere",
        "notes.pinned1",
        "notes.copy",
      ],
    );
    expect(conflicts(map, requiresOf)).toEqual([]);
    expect(heldBy(map, "Mod+1", "tab.select1", requiresOf)).toEqual([]);
    // without the requirements they would all fight
    expect(conflicts(map, ANYWHERE).map((c) => c.combo)).toEqual(["Mod+n", "Mod+1", "Mod+Shift+c"]);
    // the person's own combo on a key a narrower command holds: said (where both can run, one of them loses)
    const mine = buildKeymap(
      [
        { command: "tab.select2", keys: "Mod+2" },
        { command: "file.new", keys: "Mod+n" },
      ],
      { "notes.pinned1": "Mod+2", "notes.copy": "Mod+n" },
      ["tab.select2", "file.new", "notes.pinned1", "notes.copy"],
    );
    expect([...mine.custom]).toEqual(["notes.pinned1", "notes.copy"]);
    expect(conflicts(mine, requiresOf)).toEqual([
      { combo: "Mod+2", commands: ["tab.select2", "notes.pinned1"] },
    ]);
    expect(heldBy(mine, "Mod+2", "notes.pinned1", requiresOf)).toEqual(["tab.select2"]);
    // never in the same place: still no conflict
    expect(heldBy(mine, "Mod+n", "notes.copy", requiresOf)).toEqual([]);
  });

  it("still finds commands that can run in the same place with no rule between them", () => {
    const requires: Record<string, Requirement> = {
      a: kinds("project", "singleFile"),
      b: kinds("project", "notes"),
      c: kinds("notes"),
    };
    const requiresOf: RequiresOf = (id) => requires[id];
    const map = buildKeymap(
      [
        { command: "a", keys: "Mod+j" },
        { command: "b", keys: "Mod+j" },
        { command: "c", keys: "Mod+j" },
      ],
      {},
      ["a", "b", "c"],
    );
    // a and b overlap (project) and neither holds the other; c is inside b and apart from a
    expect(conflicts(map, requiresOf)).toEqual([{ combo: "Mod+j", commands: ["a", "b"] }]);
    expect(heldBy(map, "Mod+j", "a", requiresOf)).toEqual(["b"]);
    expect(heldBy(map, "Mod+j", "c", requiresOf)).toEqual([]);
    // the same scope twice: they fight (registration order would pick)
    expect(clash(scopeOf(kinds("notes")), scopeOf(kinds("notes")))).toBe(true);
  });

  it("reads a scope from the workspace and platform needs only", () => {
    const anywhere = scopeOf(undefined);
    expect([...anywhere.kinds].sort()).toEqual(["notes", "project", "singleFile", "welcome"]);
    expect([...anywhere.platforms].sort()).toEqual(["linux", "mac", "windows"]);
    // several workspace needs: what they all allow; other needs (git, plugin) do not narrow it
    const both: Requirement = {
      needs: [
        { need: "workspace", kinds: ["project", "notes"] },
        { need: "gitRepo" },
        { need: "workspace", kinds: ["notes", "singleFile"] },
      ],
      prefers: [],
    };
    expect([...scopeOf(both).kinds]).toEqual(["notes"]);
    // every kind listed = anywhere (neither is narrower)
    const all = scopeOf(kinds("welcome", "project", "singleFile", "notes"));
    expect(narrower(all, anywhere) || narrower(anywhere, all)).toBe(false);
    // platforms apart never fight; a platform-only command is narrower than one for every platform
    const mac = scopeOf({ needs: [{ need: "platform", platforms: ["mac"] }], prefers: [] });
    const win = scopeOf({ needs: [{ need: "platform", platforms: ["windows"] }], prefers: [] });
    expect(clash(mac, win)).toBe(false);
    expect(narrower(mac, anywhere)).toBe(true);
    // a requirement that can never hold fights nothing
    const never = scopeOf({
      needs: [
        { need: "workspace", kinds: ["project"] },
        { need: "workspace", kinds: ["notes"] },
      ],
      prefers: [],
    });
    expect(clash(never, anywhere)).toBe(false);
  });

  it("knows overrides for commands nobody registered (a plugin that is off)", () => {
    const map = buildKeymap([], { "plugin.cmd": "Mod+j" }, []);
    expect(commandsFor(map, "Mod+j")).toEqual(["plugin.cmd"]);
  });
});
