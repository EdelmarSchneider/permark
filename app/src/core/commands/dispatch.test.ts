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
    modal: false,
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
    expect(resolveKey(press({ key: "t" }), { ...base, modal: true }).kind).toBe("pass");
    expect(resolveKey(press({ key: "f", ctrlKey: true }), { ...base, typing: true }).kind).toBe(
      "run",
    );
  });

  it("while an overlay is open only the box's own commands run", () => {
    const quick = command("search.quickOpen");
    const save = command("edit.save");
    const keys = { "search.quickOpen": "Mod+p", "edit.save": "Mod+s" };
    const open = env([quick, save], keys, { modal: true });
    expect(resolveKey(press({ key: "p", ctrlKey: true }), open).kind).toBe("run");
    expect(resolveKey(press({ key: "s", ctrlKey: true }), open).kind).toBe("pass");
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
