import { describe, expect, it } from "vitest";
import { NO_REQUIREMENT, type Context, type Requirement } from "../conditions/available";
import { neutralContext } from "../conditions/context.svelte";
import { en } from "../i18n/table";
import type { Contribution } from "../plugins/registry.svelte";
import { listCommands, matchCommands, runnable, type CommandItem } from "./commands";
import { buildKeymap } from "./keymap";

const PLUGIN: Requirement = { needs: [{ need: "plugin", plugin: "git" }], prefers: [] };

function command(
  id: string,
  label: CommandItem["label"],
  extra: Partial<CommandItem> = {},
): CommandItem {
  return {
    kind: "command",
    owner: "core",
    id,
    label,
    run: () => {},
    requires: NO_REQUIREMENT,
    ...extra,
  };
}

const COMMANDS: CommandItem[] = [
  command("app.openFolder", "cmd.openFolder"),
  command("search.quickOpen", "cmd.quickOpen", { inPalette: false }),
  command("doc.find", "cmd.find", { when: () => false }),
  command("git.log", "cmd.save", { requires: PLUGIN }),
];
const ITEMS: Contribution[] = [
  ...COMMANDS,
  {
    kind: "shortcut",
    owner: "core",
    keys: "Mod+o",
    command: "app.openFolder",
    requires: NO_REQUIREMENT,
  },
];
const KEYMAP = buildKeymap(
  [{ command: "app.openFolder", keys: "Mod+o" }],
  {},
  COMMANDS.map((c) => c.id),
);
const label = (key: keyof typeof en): string => en[key];

describe("the command list", () => {
  it("hides what `when` hides and what opens the box, keeps what is unavailable", () => {
    const context: Context = neutralContext();
    const palette = listCommands(ITEMS, context, KEYMAP, { palette: true });
    expect(palette.map((e) => e.command.id)).toEqual(["app.openFolder", "git.log"]);
    expect(palette[0]?.combos).toEqual(["Mod+o"]);
    expect(palette[1]?.availability.status).toBe("unavailable");
    const all = listCommands(ITEMS, context, KEYMAP, { palette: false, all: true });
    expect(all.map((e) => e.command.id)).toEqual([
      "app.openFolder",
      "search.quickOpen",
      "doc.find",
      "git.log",
    ]);
  });

  it("says why a command is unavailable, and lists it as available once the condition holds", () => {
    const off = listCommands(ITEMS, neutralContext(), KEYMAP, { palette: true });
    const reason = off.find((e) => e.command.id === "git.log")?.availability;
    expect(reason?.status === "unavailable" && reason.reason.need).toBe("plugin");
    expect(reason?.status === "unavailable" && reason.reason.remedy).toBe("plugins.enable:git");
    const on = listCommands(ITEMS, { ...neutralContext(), plugins: ["git"] }, KEYMAP, {
      palette: true,
    });
    expect(on.find((e) => e.command.id === "git.log")?.availability.status).toBe("available");
  });

  it("decides whether a command may run now", () => {
    const find = command("doc.find", "cmd.find", { when: () => false });
    expect(runnable(find, neutralContext())).toBe(false);
    expect(runnable(command("x", "cmd.save", { requires: PLUGIN }), neutralContext())).toBe(false);
    expect(runnable(command("x", "cmd.save"), neutralContext())).toBe(true);
  });
});

describe("matching commands", () => {
  const entries = listCommands(ITEMS, neutralContext(), KEYMAP, { palette: false, all: true });

  it("keeps the order for an empty query and ranks by label for a typed one", () => {
    expect(matchCommands(entries, "  ", label).map((m) => m.entry.command.id)).toEqual(
      entries.map((e) => e.command.id),
    );
    const found = matchCommands(entries, "open", label);
    expect(found.map((m) => m.entry.command.id)).toEqual(["app.openFolder"]);
    expect(matchCommands(entries, "gofile", label).map((m) => m.entry.command.id)).toEqual([
      "search.quickOpen",
    ]);
    expect(found[0]?.positions.length).toBe(4);
    expect(matchCommands(entries, "zzzz", label)).toEqual([]);
  });

  it("a query typed as keys finds the commands on them, as each platform writes them", () => {
    const ids = (q: string): string[] =>
      matchCommands(entries, q, label).map((m) => m.entry.command.id);
    for (const q of ["ctrl+o", "Ctrl + O", "CTRL+O", "mod+o", "cmd+o", "⌘O"]) {
      expect(ids(q)[0], q).toBe("app.openFolder");
    }
    // a start of keys lists every command on them
    expect(ids("ctrl+")).toContain("app.openFolder");
    // words still match labels; keys that nothing has: nothing
    expect(ids("ctrl+alt+shift+z")).toEqual([]);
  });
});
