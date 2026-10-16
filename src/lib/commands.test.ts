import { describe, expect, it } from "vitest";
import {
  availableCommands,
  COMMANDS,
  matchCommands,
  NOT_COMMANDS,
  unregisteredActions,
  type Command,
  type CommandContext,
  type CommandLabels,
} from "./commands";
import { de } from "./i18n/de";
import { en } from "./i18n/en";
import { ptBR } from "./i18n/ptBR";
import { zhCN } from "./i18n/zhCN";
import { matchShortcut, parseCombo, SHORTCUT_ACTIONS, type KeyInfo } from "./shortcuts";

const labelsIn = (table: Record<string, string>): CommandLabels => ({
  current: (c) => table[c.label],
  english: (c) => en[c.label],
});
const english = labelsIn(en);

/** The first match of a typed query, as the label of the row. */
const top = (query: string, labels: CommandLabels = english): string | undefined =>
  matchCommands(query, COMMANDS, labels)[0]?.command.id;
const ids = (query: string, labels: CommandLabels = english): string[] =>
  matchCommands(query, COMMANDS, labels).map((m) => m.command.id);

/** The key event a combo from the registry stands for. */
function eventFor(combo: string): KeyInfo {
  const { ctrl, alt, shift, key } = parseCombo(combo);
  const named: Record<string, { key: string; code?: string }> = {
    "←": { key: "ArrowLeft" },
    "→": { key: "ArrowRight" },
    "=": { key: "=", code: "Equal" },
    "-": { key: "-", code: "Minus" },
    ",": { key: ",", code: "Comma" },
    "0": { key: "0", code: "Digit0" },
  };
  const found =
    named[key] ??
    (/^[A-Za-z]$/.test(key)
      ? { key: shift ? key.toUpperCase() : key.toLowerCase(), code: `Key${key.toUpperCase()}` }
      : { key });
  return { ...found, ctrlKey: ctrl, metaKey: false, altKey: alt, shiftKey: shift };
}

const IDLE: CommandContext = {
  ide: true,
  notes: false,
  hasDoc: false,
  movableNote: false,
  editing: false,
  canBack: false,
  canForward: false,
  webPage: false,
  hasChanges: false,
  stateInProject: false,
  hasRepo: false,
  hasUnread: false,
  agentOn: false,
  storeBuild: false,
};
const idsOf = (list: Command[]): string[] => list.map((c) => c.id);

describe("the registry", () => {
  it("accounts for every shortcut action: a command, or one of the two that open the box", () => {
    expect(unregisteredActions()).toEqual([]);
    expect(NOT_COMMANDS).toEqual(["searchEverywhere", "commands"]);
    for (const action of SHORTCUT_ACTIONS) {
      const listed = COMMANDS.some((c) => c.id === action);
      expect(listed, action).toBe(!(NOT_COMMANDS as readonly string[]).includes(action));
    }
  });

  it("lists each command once, with a label of its own", () => {
    expect(new Set(COMMANDS.map((c) => c.id)).size).toBe(COMMANDS.length);
    const labels = COMMANDS.map((c) => en[c.label]);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it("names the key that really runs the command", () => {
    const withKey = COMMANDS.filter((c) => c.shortcut !== undefined);
    expect(withKey.length).toBeGreaterThan(20);
    for (const command of withKey) {
      expect(matchShortcut(eventFor(command.shortcut ?? "")), command.id).toBe(command.id);
    }
  });

  it("gives a key to every shortcut action and to nothing else", () => {
    const shortcutIds = new Set<string>(SHORTCUT_ACTIONS);
    for (const command of COMMANDS) {
      expect(command.shortcut !== undefined, command.id).toBe(shortcutIds.has(command.id));
    }
  });
});

describe("the notes", () => {
  const NOTES: CommandContext = {
    ...IDLE,
    notes: true,
    hasDoc: true,
    hasRepo: true,
    hasUnread: true,
  };

  it("have commands in every window: a new note, the Notes window, and the whole document as text", () => {
    for (const ctx of [IDLE, { ...IDLE, ide: false }, NOTES]) {
      const ids = idsOf(availableCommands({ ...ctx, hasDoc: true }));
      expect(ids).toEqual(expect.arrayContaining(["newNote", "showNotes", "copyDocument"]));
    }
    expect(idsOf(availableCommands(IDLE))).not.toContain("copyDocument");
  });

  it("can be moved into a project only from the Notes window, with a note that has a file open", () => {
    const has = (ctx: CommandContext): boolean =>
      idsOf(availableCommands(ctx)).includes("moveToProject");
    expect(has({ ...NOTES, movableNote: true })).toBe(true);
    // a new note, a loose file or a window that only reads has none to move
    expect(has(NOTES)).toBe(false);
    // and a project has no notes
    expect(has({ ...IDLE, hasDoc: true, movableNote: true })).toBe(false);
    expect(matchShortcut(eventFor("Ctrl+Shift+S"))).toBe("moveToProject");
    expect(matchShortcut(eventFor("Ctrl+S"))).toBe("save");
  });

  it("leave out of the Notes window what belongs to a project", () => {
    const ids = idsOf(availableCommands(NOTES));
    for (const projectOnly of [
      "explorer",
      "tagsView",
      "toggleRight",
      "showComments",
      "toggleFollow",
      "nextUnread",
      "markUnread",
      "markAllRead",
      "baselineCommit",
      "baselineLastVisit",
      "agentOn",
      "initProject",
      "newFile",
      "openFolder",
      "copyReference",
      "copyPath",
      "openExternal",
      "openDefaultApp",
      "editTags",
      "addComment",
    ]) {
      expect(ids, projectOnly).not.toContain(projectOnly);
    }
    // what a note shares with a document stays: searching the notes, editing, the list, the zoom; and a file can be opened
    expect(ids).toEqual(
      expect.arrayContaining([
        "quickOpen",
        "searchAll",
        "toggleLeft",
        "toggleEdit",
        "zoomIn",
        "openFile",
      ]),
    );
    // and a project window still has them all
    expect(idsOf(availableCommands({ ...IDLE, hasDoc: true, hasRepo: true }))).toEqual(
      expect.arrayContaining([
        "explorer",
        "toggleRight",
        "openFolder",
        "copyReference",
        "copyPath",
      ]),
    );
  });
});

describe("availableCommands", () => {
  it("offers Save only while editing", () => {
    expect(idsOf(availableCommands({ ...IDLE, hasDoc: true }))).not.toContain("save");
    expect(idsOf(availableCommands({ ...IDLE, hasDoc: true, editing: true }))).toContain("save");
  });

  it("offers next and previous change only for a document with changes, in the reading view", () => {
    const withChanges = { ...IDLE, hasDoc: true, hasChanges: true };
    expect(idsOf(availableCommands({ ...IDLE, hasDoc: true }))).not.toContain("nextChange");
    expect(idsOf(availableCommands(withChanges))).toEqual(
      expect.arrayContaining(["nextChange", "prevChange"]),
    );
    expect(idsOf(availableCommands({ ...withChanges, editing: true }))).not.toContain("nextChange");
  });

  it("offers Back and Forward only when there is somewhere to go", () => {
    expect(idsOf(availableCommands(IDLE))).not.toContain("back");
    expect(idsOf(availableCommands({ ...IDLE, canBack: true }))).toContain("back");
    expect(idsOf(availableCommands({ ...IDLE, canBack: true }))).not.toContain("forward");
    expect(idsOf(availableCommands({ ...IDLE, canForward: true }))).toContain("forward");
  });

  it('offers "Go to a recent place" when there is a trail to look at, in a folder or a single file', () => {
    expect(idsOf(availableCommands(IDLE))).not.toContain("trail");
    expect(idsOf(availableCommands({ ...IDLE, canBack: true }))).toContain("trail");
    expect(idsOf(availableCommands({ ...IDLE, canForward: true }))).toContain("trail");
    expect(idsOf(availableCommands({ ...IDLE, ide: false, canBack: true }))).toContain("trail");
    // a note is not a page to go back to
    expect(idsOf(availableCommands({ ...IDLE, notes: true, canBack: true }))).not.toContain(
      "trail",
    );
  });

  it('offers "Open in browser" for a web page only', () => {
    expect(idsOf(availableCommands({ ...IDLE, hasDoc: true }))).not.toContain("openInBrowser");
    expect(idsOf(availableCommands({ ...IDLE, hasDoc: true, webPage: true }))).toContain(
      "openInBrowser",
    );
    // a single file can be a web page too
    expect(idsOf(availableCommands({ ...IDLE, ide: false, webPage: true }))).toContain(
      "openInBrowser",
    );
    expect(idsOf(availableCommands({ ...IDLE, notes: true, webPage: true }))).not.toContain(
      "openInBrowser",
    );
  });

  it("offers the commands of the open document only when there is one", () => {
    const without = idsOf(availableCommands(IDLE));
    for (const id of [
      "copyPath",
      "copyReference",
      "openExternal",
      "openDefaultApp",
      "find",
      "edit",
      "revealInTree",
    ]) {
      expect(without, id).not.toContain(id);
    }
    const open = idsOf(availableCommands({ ...IDLE, hasDoc: true }));
    for (const id of [
      "copyPath",
      "copyReference",
      "openExternal",
      "openDefaultApp",
      "find",
      "edit",
      "revealInTree",
    ]) {
      expect(open, id).toContain(id);
    }
  });

  it("offers the comments in a folder: adding one to the selection while reading, showing them always", () => {
    const folder = idsOf(availableCommands(IDLE));
    expect(folder).toContain("showComments");
    expect(folder).not.toContain("addComment");
    expect(idsOf(availableCommands({ ...IDLE, hasDoc: true }))).toContain("addComment");
    expect(idsOf(availableCommands({ ...IDLE, hasDoc: true, editing: true }))).not.toContain(
      "addComment",
    );
    // a single file has no project to keep comments in
    const simple = idsOf(availableCommands({ ...IDLE, ide: false, hasDoc: true }));
    expect(simple).not.toContain("addComment");
    expect(simple).not.toContain("showComments");
  });

  it("offers Initialize until the state is kept in the project", () => {
    expect(idsOf(availableCommands(IDLE))).toContain("initProject");
    expect(idsOf(availableCommands({ ...IDLE, stateInProject: true }))).not.toContain(
      "initProject",
    );
  });

  it('offers "Check for updates" everywhere except the Microsoft Store build, which the Store updates', () => {
    expect(idsOf(availableCommands(IDLE))).toContain("checkUpdates");
    expect(idsOf(availableCommands({ ...IDLE, ide: false }))).toContain("checkUpdates");
    const store = { ...IDLE, storeBuild: true };
    expect(idsOf(availableCommands(store))).not.toContain("checkUpdates");
    expect(idsOf(availableCommands({ ...store, ide: false, hasDoc: true }))).not.toContain(
      "checkUpdates",
    );
    // the rest of the list is the same
    const others = (ctx: CommandContext): string[] =>
      idsOf(availableCommands(ctx)).filter((id) => id !== "checkUpdates");
    expect(others(store)).toEqual(others(IDLE));
  });

  it("offers the agent connection's switch for the state it is in, and its setup in a folder", () => {
    const off = idsOf(availableCommands(IDLE));
    expect(off).toContain("agentOn");
    expect(off).not.toContain("agentOff");
    const on = idsOf(availableCommands({ ...IDLE, agentOn: true }));
    expect(on).toContain("agentOff");
    expect(on).not.toContain("agentOn");
    expect(on).toContain("agentCopySetup");
    // a single file is no project: nothing of it applies
    const simple = idsOf(availableCommands({ ...IDLE, ide: false }));
    for (const id of ["agentOn", "agentOff", "agentCopySetup"]) {
      expect(simple, id).not.toContain(id);
    }
  });

  it("offers the git baselines only in a repository", () => {
    expect(idsOf(availableCommands(IDLE))).not.toContain("baselineCommit");
    const repo = idsOf(availableCommands({ ...IDLE, hasRepo: true }));
    expect(repo).toEqual(expect.arrayContaining(["baselineLastVisit", "baselineCommit"]));
  });

  it("leaves the folder commands out of Simple mode, where only the file's own commands remain", () => {
    const simple = idsOf(availableCommands({ ...IDLE, ide: false, hasDoc: true }));
    for (const id of ["quickOpen", "searchAll", "explorer", "tagsView", "newFile", "newFolder"]) {
      expect(simple, id).not.toContain(id);
    }
    expect(simple).toEqual(
      expect.arrayContaining([
        "openContainingFolder",
        "zoomIn",
        "settings",
        "checkUpdates",
        "edit",
      ]),
    );
    expect(idsOf(availableCommands(IDLE))).not.toContain("openContainingFolder");
  });

  it("keeps the registry order", () => {
    const all = idsOf(availableCommands(IDLE));
    const registry = idsOf([...COMMANDS]).filter((id) => all.includes(id));
    expect(all).toEqual(registry);
  });
});

describe("matchCommands", () => {
  it("lists every command, in order, for an empty query", () => {
    const found = matchCommands("  ", COMMANDS, english);
    expect(found.map((m) => m.command.id)).toEqual(COMMANDS.map((c) => c.id));
    expect(found[0].positions).toEqual([]);
    expect(found[0].alias).toBeNull();
  });

  it("finds a command by the words of its label, in any order", () => {
    expect(top("toggle outline")).toBe("toggleOutline");
    expect(top("outline toggle")).toBe("toggleOutline");
    expect(top("upd")).toBe("checkUpdates");
    expect(ids("initialize")).toEqual(["initProject"]);
  });

  it("ranks a label that starts with the query above one that only contains it", () => {
    const found = ids("save");
    expect(found[0]).toBe("save");
  });

  it("ranks the start of a word above the middle of one", () => {
    // "path" starts a word of "Copy path"; it is inside the label of no other command
    expect(top("path")).toBe("copyPath");
    const found = matchCommands("fol", COMMANDS, english);
    expect(en[found[0].command.label].toLowerCase()).toMatch(/\bfol/);
  });

  it("ranks a shorter label first, and keeps the registry order between equal ones", () => {
    expect(ids("zoom")).toEqual(["zoomIn", "zoomOut", "zoomReset"]);
    const same: CommandLabels = { current: () => "Go there", english: () => "Go there" };
    const command = (id: string): Command => COMMANDS.find((c) => c.id === id) as Command;
    const two = [command("forward"), command("quickOpen")];
    expect(matchCommands("go", two, same).map((m) => m.command.id)).toEqual([
      "forward",
      "quickOpen",
    ]);
  });

  it("lists the letters in order only when no label has the words", () => {
    // "tog" is scattered over "Go to next change" but starts "Toggle ...": only the latter are listed
    const found = ids("tog");
    expect(found).toContain("toggleOutline");
    expect(found).not.toContain("nextChange");
    expect(ids("gnc")).toContain("nextChange");
  });

  it("matches letters in order when no word fits", () => {
    expect(ids("tglol")).toContain("toggleOutline");
    expect(ids("xyzzy")).toEqual([]);
  });

  it("ignores case and accents", () => {
    expect(top("TOGGLE OUTLINE")).toBe("toggleOutline");
    expect(top("configuracoes", labelsIn(ptBR))).toBe("settings");
    expect(top("CONFIGURAÇÕES", labelsIn(ptBR))).toBe("settings");
  });

  it("returns nothing for a query no label has", () => {
    expect(ids("xyzzy")).toEqual([]);
  });

  it("points at the matched characters of the label it shows", () => {
    const [match] = matchCommands("outline", COMMANDS, english);
    const label = en[match.command.label];
    expect(match.positions.map((p) => label[p]).join("")).toBe("outline");
    expect(match.alias).toBeNull();
  });

  describe("in another language", () => {
    const pt = labelsIn(ptBR);

    it("matches the label in the current language", () => {
      expect(ptBR[COMMANDS[0].label]).not.toBe(en[COMMANDS[0].label]);
      const found = matchCommands("salvar", COMMANDS, pt);
      expect(found[0].command.id).toBe("save");
      expect(found[0].alias).toBeNull();
      expect(found[0].label).toBe(ptBR["cmd.save"]);
      expect(found[0].positions.length).toBeGreaterThan(0);
    });

    it("still matches the English label, and says so", () => {
      const found = matchCommands("toggle outline", COMMANDS, pt);
      expect(found[0].command.id).toBe("toggleOutline");
      expect(found[0].label).toBe(ptBR["cmd.toggleOutline"]);
      expect(found[0].alias).toBe(en["cmd.toggleOutline"]);
      expect(found[0].positions).toEqual([]);
    });

    it("points at the current label, with no alias, when it matches too", () => {
      const found = matchCommands("zoom", COMMANDS, pt);
      expect(found.map((m) => m.command.id)).toEqual(
        expect.arrayContaining(["zoomIn", "zoomOut", "zoomReset"]),
      );
      for (const match of found) {
        expect(match.alias, match.command.id).toBeNull();
        expect(match.positions.length, match.command.id).toBeGreaterThan(0);
      }
    });

    it("finds a command by Chinese characters, which have no spaces to start words at", () => {
      const zh = labelsIn(zhCN);
      expect(top("保存", zh)).toBe("save");
      expect(top("大纲", zh)).toBe("toggleOutline");
      // the English names keep working
      expect(top("toggle outline", zh)).toBe("toggleOutline");
    });

    it("lists a command once, whichever language matched", () => {
      const found = ids("settings", labelsIn(de));
      expect(found.filter((id) => id === "settings")).toHaveLength(1);
      expect(top("einstellungen", labelsIn(de))).toBe("settings");
      expect(top("settings", labelsIn(de))).toBe("settings");
    });
  });
});
