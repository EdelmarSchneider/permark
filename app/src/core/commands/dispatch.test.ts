// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import type { Context, Requirement } from "../conditions/available";
import { NO_REQUIREMENT } from "../conditions/available";
import { neutralContext } from "../conditions/context.svelte";
import type { CommandItem } from "./commands";
import {
  installButtons,
  installKeys,
  isStraySelectAll,
  isTextTarget,
  isTypingTarget,
  resolveButton,
  resolveKey,
  type ButtonEnv,
  type KeyEnv,
} from "./dispatch";
import { buildKeymap } from "./keymap";

function command(id: string, extra: Partial<CommandItem> = {}): CommandItem {
  return {
    kind: "command",
    owner: "core",
    id,
    label: "cmd.save",
    run: () => {},
    requires: NO_REQUIREMENT,
    ...extra,
  };
}

const PROJECT: Requirement = { needs: [{ need: "workspace", kinds: ["project"] }], prefers: [] };

function env(
  commands: CommandItem[],
  keys: Record<string, string>,
  extra: Partial<KeyEnv> = {},
): KeyEnv {
  const context: Context = { ...neutralContext(), workspace: "project" };
  return {
    mac: false,
    keymap: buildKeymap(
      Object.entries(keys).map(([id, combo]) => ({ command: id, keys: combo })),
      {},
      commands.map((c) => c.id),
    ),
    commands,
    context,
    typing: false,
    overlay: "none",
    dialog: false,
    ...extra,
  };
}

function press(init: Partial<KeyboardEventInit> & { key: string }): KeyboardEvent {
  return new KeyboardEvent("keydown", { cancelable: true, bubbles: true, ...init });
}

describe("key -> command", () => {
  it("runs the command that holds the combo", () => {
    const save = command("edit.save");
    const decision = resolveKey(
      press({ key: "s", ctrlKey: true }),
      env([save], { "edit.save": "Mod+s" }),
    );
    expect(decision).toEqual({ kind: "run", command: save, combo: "Mod+s" });
  });

  it("tells F7 from Shift+F7, and leaves them to a field while the person types", () => {
    const next = command("doc.nextChange");
    const prev = command("doc.prevChange");
    const e = env([next, prev], { "doc.nextChange": "F7", "doc.prevChange": "Shift+F7" });
    expect(resolveKey(press({ key: "F7" }), e)).toMatchObject({ kind: "run", command: next });
    expect(resolveKey(press({ key: "F7", shiftKey: true }), e)).toMatchObject({
      kind: "run",
      command: prev,
    });
    expect(resolveKey(press({ key: "F7" }), { ...e, typing: true }).kind).toBe("pass");
  });

  it("F5 reaches its command while the person types too: it is never text, and the webview would reload on it", () => {
    const refresh = command("explorer.refresh", { requires: PROJECT });
    const next = command("doc.nextChange");
    const e = env([refresh, next], { "explorer.refresh": "F5", "doc.nextChange": "F7" });
    expect(resolveKey(press({ key: "F5" }), e)).toMatchObject({ kind: "run", command: refresh });
    expect(resolveKey(press({ key: "F5" }), { ...e, typing: true })).toMatchObject({
      kind: "run",
      command: refresh,
    });
    // the other plain keys still yield to a field
    expect(resolveKey(press({ key: "F7" }), { ...e, typing: true }).kind).toBe("pass");
    // a box | a question | a dialog owns the input, or no project is shown: the command does not run
    expect(resolveKey(press({ key: "F5" }), { ...e, overlay: "box" }).kind).toBe("pass");
    expect(resolveKey(press({ key: "F5" }), { ...e, overlay: "question" }).kind).toBe("pass");
    expect(resolveKey(press({ key: "F5" }), { ...e, dialog: true }).kind).toBe("pass");
    expect(resolveKey(press({ key: "F5" }), { ...e, context: neutralContext() }).kind).toBe("pass");
  });

  it("passes keys nobody holds, keys another handler took and compositions", () => {
    const save = command("edit.save");
    const e = env([save], { "edit.save": "Mod+s" });
    expect(resolveKey(press({ key: "x", ctrlKey: true }), e).kind).toBe("pass");
    const taken = press({ key: "s", ctrlKey: true });
    taken.preventDefault();
    expect(resolveKey(taken, e).kind).toBe("pass");
    expect(resolveKey(press({ key: "s", ctrlKey: true, isComposing: true }), e).kind).toBe("pass");
    expect(resolveKey(press({ key: "Dead", ctrlKey: true }), e).kind).toBe("pass");
  });

  it("lets plain keys be text while the person types or an overlay is open, and chords always run", () => {
    const tags = command("tags.edit");
    const find = command("doc.find");
    const keys = { "tags.edit": "t", "doc.find": "Mod+f" };
    const base = env([tags, find], keys);
    expect(resolveKey(press({ key: "t" }), base).kind).toBe("run");
    expect(resolveKey(press({ key: "t" }), { ...base, typing: true }).kind).toBe("pass");
    expect(resolveKey(press({ key: "t" }), { ...base, overlay: "box" }).kind).toBe("pass");
    expect(resolveKey(press({ key: "f", ctrlKey: true }), { ...base, typing: true }).kind).toBe(
      "run",
    );
  });

  it("while an overlay is open only the box's own commands run", () => {
    const quick = command("search.quickOpen");
    const save = command("edit.save");
    const keys = { "search.quickOpen": "Mod+p", "edit.save": "Mod+s" };
    const open = env([quick, save], keys, { overlay: "box" });
    expect(resolveKey(press({ key: "p", ctrlKey: true }), open).kind).toBe("run");
    expect(resolveKey(press({ key: "s", ctrlKey: true }), open).kind).toBe("pass");
  });

  it("while a question is up (the window close prompt) no command runs, not even the tab keys", () => {
    const next = command("tab.next");
    const quick = command("search.quickOpen");
    const keys = { "tab.next": "Ctrl+Tab", "search.quickOpen": "Mod+p" };
    const asking = env([next, quick], keys, { overlay: "question" });
    expect(resolveKey(press({ key: "Tab", ctrlKey: true }), asking).kind).toBe("pass");
    expect(resolveKey(press({ key: "p", ctrlKey: true }), asking).kind).toBe("pass");
    // with a box open the tab keys still switch
    const box = env([next, quick], keys, { overlay: "box" });
    expect(resolveKey(press({ key: "Tab", ctrlKey: true }), box).kind).toBe("run");
  });

  it("while a modal dialog is open (the unsaved changes question) no command runs: not save, not close tab", () => {
    const save = command("edit.save");
    const close = command("tab.close");
    const quick = command("search.quickOpen");
    const keys = { "edit.save": "Mod+s", "tab.close": "Mod+w", "search.quickOpen": "Mod+p" };
    const asking = env([save, close, quick], keys, { dialog: true });
    expect(resolveKey(press({ key: "s", ctrlKey: true }), asking).kind).toBe("pass");
    expect(resolveKey(press({ key: "w", ctrlKey: true }), asking).kind).toBe("pass");
    expect(resolveKey(press({ key: "p", ctrlKey: true }), asking).kind).toBe("pass");
    // a box is a modal dialog too: its own commands still run
    const box = env([save, close, quick], keys, { dialog: true, overlay: "box" });
    expect(resolveKey(press({ key: "p", ctrlKey: true }), box).kind).toBe("run");
    expect(resolveKey(press({ key: "s", ctrlKey: true }), box).kind).toBe("pass");
  });

  it("skips a command whose condition is unmet or whose `when` says no, and tries the next holder", () => {
    const gated = command("a.git", { requires: PROJECT });
    const hidden = command("b.hidden", { when: () => false });
    const fallback = command("c.fallback");
    const keys = { "a.git": "Mod+k", "b.hidden": "Mod+k", "c.fallback": "Mod+k" };
    const welcome: Context = neutralContext();
    const e = env([gated, hidden, fallback], keys, { context: welcome });
    const decision = resolveKey(press({ key: "k", ctrlKey: true }), e);
    expect(decision.kind === "run" && decision.command.id).toBe("c.fallback");
    const inProject = env([gated, hidden, fallback], keys);
    const second = resolveKey(press({ key: "k", ctrlKey: true }), inProject);
    expect(second.kind === "run" && second.command.id).toBe("a.git");
  });

  it("in the Notes tab the Notes' own keys win over the wider ones; elsewhere the wider ones run", () => {
    const NOTES: Requirement = { needs: [{ need: "workspace", kinds: ["notes"] }], prefers: [] };
    const READING: Requirement = {
      needs: [{ need: "workspace", kinds: ["project", "singleFile", "notes"] }],
      prefers: [],
    };
    // registration order: the core's first (as in the app), the plugin's after
    const tab1 = command("tab.select1");
    const last = command("tab.selectLast");
    const fileNew = command("file.new", { requires: PROJECT });
    const copyDoc = command("doc.copyDocument", { requires: READING });
    const pinned1 = command("notes.pinned1", { requires: NOTES });
    const pinned9 = command("notes.pinned9", { requires: NOTES });
    const newHere = command("notes.newHere", { requires: NOTES });
    const copyNote = command("notes.copy", { requires: NOTES });
    const all = [tab1, last, fileNew, copyDoc, pinned1, pinned9, newHere, copyNote];
    const keys = {
      "tab.select1": "Mod+1",
      "tab.selectLast": "Mod+9",
      "file.new": "Mod+n",
      "doc.copyDocument": "Mod+Shift+c",
      "notes.pinned1": "Mod+1",
      "notes.pinned9": "Mod+9",
      "notes.newHere": "Mod+n",
      "notes.copy": "Mod+Shift+c",
    };
    const at = (workspace: Context["workspace"]): KeyEnv => ({
      ...env(all, keys),
      context: { ...neutralContext(), workspace },
    });
    const run = (e: KeyEnv, init: Partial<KeyboardEventInit> & { key: string }) => {
      const decision = resolveKey(press({ ctrlKey: true, ...init }), e);
      return decision.kind === "run" ? decision.command.id : null;
    };
    const notes = at("notes");
    expect(run(notes, { key: "1" })).toBe("notes.pinned1");
    expect(run(notes, { key: "9" })).toBe("notes.pinned9");
    expect(run(notes, { key: "n" })).toBe("notes.newHere");
    expect(run(notes, { key: "C", shiftKey: true })).toBe("notes.copy");
    const project = at("project");
    expect(run(project, { key: "1" })).toBe("tab.select1");
    expect(run(project, { key: "9" })).toBe("tab.selectLast");
    expect(run(project, { key: "n" })).toBe("file.new");
    expect(run(project, { key: "C", shiftKey: true })).toBe("doc.copyDocument");
    // the narrower one cannot run now (its `when` says no): the wider one still does
    const hidden = { ...copyNote, when: () => false };
    const fallback: KeyEnv = {
      ...env([tab1, last, fileNew, copyDoc, pinned1, pinned9, newHere, hidden], keys),
      context: { ...neutralContext(), workspace: "notes" },
    };
    expect(run(fallback, { key: "C", shiftKey: true })).toBe("doc.copyDocument");
    // registered the other way round: the same choice (narrower, not first)
    const reversed: KeyEnv = {
      ...env([...all].reverse(), keys),
      context: { ...neutralContext(), workspace: "notes" },
    };
    expect(run(reversed, { key: "1" })).toBe("notes.pinned1");
    expect(run(at("singleFile"), { key: "C", shiftKey: true })).toBe("doc.copyDocument");
  });

  it("uses Cmd on macOS", () => {
    const save = command("edit.save");
    const e = env([save], { "edit.save": "Mod+s" }, { mac: true });
    expect(resolveKey(press({ key: "s", metaKey: true }), e).kind).toBe("run");
    expect(resolveKey(press({ key: "s", ctrlKey: true }), e).kind).toBe("pass");
  });
});

describe("the listener", () => {
  it("runs the command, takes the key, survives a throwing command, and stops when told", () => {
    const run = vi.fn();
    const boom = vi.fn(() => {
      throw new Error("boom");
    });
    const e = env([command("a.ok", { run }), command("b.boom", { run: boom })], {
      "a.ok": "Mod+j",
      "b.boom": "Mod+b",
    });
    const stop = installKeys(window, () => e);
    const ok = press({ key: "j", ctrlKey: true });
    window.dispatchEvent(ok);
    expect(run).toHaveBeenCalledOnce();
    expect(ok.defaultPrevented).toBe(true);
    window.dispatchEvent(press({ key: "b", ctrlKey: true }));
    expect(boom).toHaveBeenCalledOnce();
    window.dispatchEvent(press({ key: "j", ctrlKey: true }));
    expect(run).toHaveBeenCalledTimes(2);
    const free = press({ key: "q", ctrlKey: true });
    window.dispatchEvent(free);
    expect(free.defaultPrevented).toBe(false);
    stop();
    window.dispatchEvent(press({ key: "j", ctrlKey: true }));
    expect(run).toHaveBeenCalledTimes(2);
  });

  it("never leaves F5 to the webview: with no command to run the key is taken and nothing happens", () => {
    const run = vi.fn();
    const e = env([command("explorer.refresh", { run })], { "explorer.refresh": "F5" });
    let now: KeyEnv = e;
    const stop = installKeys(window, () => now);
    const ran = press({ key: "F5" });
    window.dispatchEvent(ran);
    expect(run).toHaveBeenCalledOnce();
    expect(ran.defaultPrevented).toBe(true);
    // a dialog owns the input | nobody holds the key (with any modifier): no reload of the page either
    for (const state of [{ ...e, dialog: true }, env([], {})]) {
      now = state;
      for (const init of [{}, { ctrlKey: true }, { shiftKey: true }]) {
        const swallowed = press({ key: "F5", ...init });
        window.dispatchEvent(swallowed);
        expect(swallowed.defaultPrevented).toBe(true);
      }
    }
    expect(run).toHaveBeenCalledOnce();
    // other keys nobody holds stay the webview's
    const free = press({ key: "F3" });
    window.dispatchEvent(free);
    expect(free.defaultPrevented).toBe(false);
    stop();
    const after = press({ key: "F5" });
    window.dispatchEvent(after);
    expect(after.defaultPrevented).toBe(false);
  });

  it("never leaves select-all to the webview where no text has the keyboard: the key is taken and nothing runs", () => {
    document.body.innerHTML =
      '<p id="words">words</p><button id="button">b</button><input type="checkbox" id="box">' +
      '<select id="choice"><option>x</option></select><input id="field"><textarea id="area"></textarea>' +
      '<div contenteditable="true" id="edit"><p id="inside">x</p></div><div class="cm-content" id="source"></div>';
    const from = (id: string, init: Partial<KeyboardEventInit> = {}): KeyboardEvent => {
      const event = press({ key: "a", code: "KeyA", ctrlKey: true, ...init });
      document.getElementById(id)?.dispatchEvent(event);
      return event;
    };
    const run = vi.fn();
    const held = env([command("doc.selectAll", { run })], { "doc.selectAll": "Mod+a" });
    let now: KeyEnv = held;
    const stop = installKeys(window, () => now);
    expect(from("words").defaultPrevented).toBe(true);
    expect(run).toHaveBeenCalledOnce();
    // a dialog | a question | a box owns the input, the window has no such command, the person gave it another key
    const states: KeyEnv[] = [
      { ...held, dialog: true },
      { ...held, overlay: "question" },
      { ...held, overlay: "box", dialog: true },
      {
        ...held,
        context: neutralContext(),
        commands: [command("doc.selectAll", { requires: PROJECT })],
      },
      env([], {}),
    ];
    for (const [at, state] of states.entries()) {
      now = state;
      for (const id of ["words", "button", "box", "choice"]) {
        expect(from(id).defaultPrevented, `${at} ${id}`).toBe(true);
      }
      // in text it stays the text's own select-all
      for (const id of ["field", "area", "edit", "inside", "source"]) {
        expect(from(id).defaultPrevented, `${at} ${id}`).toBe(false);
      }
      // only that chord: the other keys nobody holds stay the webview's
      expect(from("words", { shiftKey: true }).defaultPrevented).toBe(false);
      expect(from("words", { altKey: true }).defaultPrevented).toBe(false);
      expect(from("words", { key: "c", code: "KeyC" }).defaultPrevented).toBe(false);
      expect(from("words", { ctrlKey: false }).defaultPrevented).toBe(false);
    }
    expect(run).toHaveBeenCalledOnce();
    // macOS: Cmd+A, not Control+A
    now = { ...env([], {}), mac: true };
    expect(from("words", { ctrlKey: false, metaKey: true }).defaultPrevented).toBe(true);
    expect(from("words").defaultPrevented).toBe(false);
    stop();
    expect(from("words").defaultPrevented).toBe(false);
  });

  it("tells a select-all press outside text by the chord and by where the keyboard is, on any layout", () => {
    document.body.innerHTML = '<p id="words">words</p><input id="field">';
    const words = document.getElementById("words");
    const field = document.getElementById("field");
    const chord = { key: "a", code: "KeyA", ctrlKey: true, metaKey: false, altKey: false };
    expect(isStraySelectAll({ ...chord, shiftKey: false, target: words }, false)).toBe(true);
    expect(isStraySelectAll({ ...chord, shiftKey: false, target: null }, false)).toBe(true);
    expect(isStraySelectAll({ ...chord, shiftKey: false, target: field }, false)).toBe(false);
    expect(isStraySelectAll({ ...chord, shiftKey: true, target: words }, false)).toBe(false);
    // Caps Lock | another layout (Cyrillic: the same physical key)
    expect(isStraySelectAll({ ...chord, key: "A", shiftKey: false, target: words }, false)).toBe(
      true,
    );
    expect(isStraySelectAll({ ...chord, key: "ф", shiftKey: false, target: words }, false)).toBe(
      true,
    );
    // a composition | AltGr types characters
    expect(
      isStraySelectAll({ ...chord, shiftKey: false, isComposing: true, target: words }, false),
    ).toBe(false);
  });

  it("a held key runs Refresh once: its repeats are taken and run nothing; a command that steps follows the key", () => {
    const refresh = vi.fn();
    const next = vi.fn();
    const commands = [
      command("explorer.refresh", { run: refresh }),
      command("doc.nextChange", { run: next }),
    ];
    let now = env(commands, { "explorer.refresh": "F5", "doc.nextChange": "F7" });
    const stop = installKeys(window, () => now);
    window.dispatchEvent(press({ key: "F5" }));
    for (let n = 0; n < 3; n += 1) {
      const held = press({ key: "F5", repeat: true });
      window.dispatchEvent(held);
      expect(held.defaultPrevented).toBe(true);
    }
    expect(refresh).toHaveBeenCalledOnce();
    // released and pressed again
    window.dispatchEvent(press({ key: "F5" }));
    expect(refresh).toHaveBeenCalledTimes(2);
    // the same on a key the person gave it: taken (never the webview's own reload), not run
    now = env(commands, { "explorer.refresh": "Mod+r", "doc.nextChange": "F7" });
    const other = press({ key: "r", ctrlKey: true, repeat: true });
    window.dispatchEvent(other);
    expect(other.defaultPrevented).toBe(true);
    expect(refresh).toHaveBeenCalledTimes(2);
    window.dispatchEvent(press({ key: "F7" }));
    window.dispatchEvent(press({ key: "F7", repeat: true }));
    expect(next).toHaveBeenCalledTimes(2);
    stop();
  });
});

describe("mouse side buttons", () => {
  const READING: Requirement = {
    needs: [{ need: "workspace", kinds: ["project", "singleFile", "notes"] }],
    prefers: [],
  };
  function buttons(extra: Partial<ButtonEnv> = {}): {
    env: ButtonEnv;
    back: ReturnType<typeof vi.fn>;
    forward: ReturnType<typeof vi.fn>;
  } {
    const back = vi.fn();
    const forward = vi.fn();
    // `when` false: like Alt+Left inside an editor; a side button is never text, so it still runs
    const env: ButtonEnv = {
      commands: [
        command("doc.back", { run: back, requires: READING, when: () => false }),
        command("doc.forward", { run: forward, requires: READING, when: () => false }),
      ],
      context: { ...neutralContext(), workspace: "project" },
      overlay: "none",
      dialog: false,
      ...extra,
    };
    return { env, back, forward };
  }
  const up = (button: number): MouseEvent =>
    new MouseEvent("mouseup", { button, bubbles: true, cancelable: true });

  it("button 3 runs back, button 4 forward; 0-2 and unknown buttons pass", () => {
    const { env } = buttons();
    expect(resolveButton(up(3), env)).toMatchObject({ kind: "run", command: { id: "doc.back" } });
    expect(resolveButton(up(4), env)).toMatchObject({
      kind: "run",
      command: { id: "doc.forward" },
    });
    for (const b of [0, 1, 2, 5, -1])
      expect(resolveButton(up(b), env).kind, `button ${b}`).toBe("pass");
  });

  it("passes when an overlay or a dialog owns the input, another handler took it, or nothing shows a document", () => {
    expect(resolveButton(up(3), buttons({ overlay: "box" }).env).kind).toBe("pass");
    expect(resolveButton(up(3), buttons({ overlay: "question" }).env).kind).toBe("pass");
    expect(resolveButton(up(4), buttons({ dialog: true }).env).kind).toBe("pass");
    const taken = up(3);
    taken.preventDefault();
    expect(resolveButton(taken, buttons().env).kind).toBe("pass");
    const welcome = buttons({ context: { ...neutralContext(), workspace: "welcome" } }).env;
    expect(resolveButton(up(3), welcome).kind).toBe("pass");
    expect(resolveButton(up(3), buttons({ commands: [] }).env).kind).toBe("pass");
  });

  it("the listener: one step per release (mousedown, auxclick do nothing), takes it, survives a throw, stops", () => {
    const { env, back, forward } = buttons();
    const stop = installButtons(window, () => env);
    window.dispatchEvent(new MouseEvent("mousedown", { button: 3, bubbles: true }));
    const release = up(3);
    window.dispatchEvent(release);
    window.dispatchEvent(new MouseEvent("auxclick", { button: 3, bubbles: true }));
    expect(back).toHaveBeenCalledOnce();
    expect(release.defaultPrevented).toBe(true);
    const left = up(0);
    window.dispatchEvent(left);
    expect(left.defaultPrevented).toBe(false);
    forward.mockImplementation(() => {
      throw new Error("boom");
    });
    window.dispatchEvent(up(4));
    window.dispatchEvent(up(3));
    expect(forward).toHaveBeenCalledOnce();
    expect(back).toHaveBeenCalledTimes(2);
    stop();
    window.dispatchEvent(up(3));
    expect(back).toHaveBeenCalledTimes(2);
  });
});

describe("where typing goes", () => {
  it("knows fields and editors", () => {
    document.body.innerHTML =
      '<input id="i"><textarea id="t"></textarea><div contenteditable="true" id="c"><b id="in">x</b></div><div class="cm-content" id="cm"><span id="cs">y</span></div><button id="b"></button>';
    const get = (id: string): Element | null => document.getElementById(id);
    for (const id of ["i", "t", "c", "in", "cm", "cs"]) {
      expect(isTypingTarget(get(id)), id).toBe(true);
    }
    expect(isTypingTarget(get("b"))).toBe(false);
    expect(isTypingTarget(document.body)).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });

  it("knows text from the controls that only take keys: a box to tick, a slider, a choice list hold none", () => {
    const kinds = ["checkbox", "radio", "button", "submit", "reset", "range", "color", "file"];
    document.body.innerHTML =
      '<input id="plain"><input type="text" id="text"><input type="search" id="search">' +
      '<input type="password" id="password"><input type="number" id="number"><input type="TEXT" id="upper">' +
      '<textarea id="area"></textarea><div contenteditable="true" id="edit"><b id="in">x</b>' +
      '<input type="checkbox" id="task"></div><div class="cm-content" id="cm"><span id="cs">y</span></div>' +
      '<select id="choice"><option>x</option></select><button id="b"></button><p id="p">words</p>' +
      kinds.map((kind) => `<input type="${kind}" id="${kind}">`).join("") +
      '<input type="CheckBox" id="mixed">';
    const get = (id: string): Element | null => document.getElementById(id);
    const text = ["plain", "text", "search", "password", "number", "upper", "area", "edit", "in"];
    for (const id of [...text, "cm", "cs"]) expect(isTextTarget(get(id)), id).toBe(true);
    // a task box inside the editor is the editor's: select-all there selects its text
    expect(isTextTarget(get("task"))).toBe(true);
    for (const id of [...kinds, "mixed", "choice", "b", "p"]) {
      expect(isTextTarget(get(id)), id).toBe(false);
    }
    expect(isTextTarget(document.body)).toBe(false);
    expect(isTextTarget(null)).toBe(false);
    // plain keys are still theirs (Space ticks, arrows move, letters jump in a list): typing as before
    for (const id of [...kinds, "mixed", "choice"]) expect(isTypingTarget(get(id)), id).toBe(true);
  });
});
