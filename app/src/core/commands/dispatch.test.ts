// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import type { Context, Requirement } from "../conditions/available";
import { NO_REQUIREMENT } from "../conditions/available";
import { neutralContext } from "../conditions/context.svelte";
import type { CommandItem } from "./commands";
import { installKeys, isTypingTarget, resolveKey, type KeyEnv } from "./dispatch";
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
});
