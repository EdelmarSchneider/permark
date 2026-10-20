// Key -> command. One `keydown` listener per window; the decision is a pure function so every guard is tested.

import type { Context } from "../conditions/available";
import { comboFromEvent, isSingleKey, type KeyEventLike } from "./combo";
import { runnable, type CommandItem } from "./commands";
import { commandsFor, narrower, scopeOf, type Keymap } from "./keymap";

export type KeyEnv = {
  mac: boolean;
  keymap: Keymap;
  /** Every registered command, in registration order. */
  commands: readonly CommandItem[];
  context: Context;
  /** An editable element has the focus: plain keys are text there. */
  typing: boolean;
  /** What owns the keyboard: nothing, a box (Search everywhere, the keyboard dialog: its own commands still run),
   * a question to answer (the window close prompt: no command runs). */
  overlay: Overlay;
};

export type Overlay = "none" | "box" | "question";

export type KeyDecision = { kind: "run"; command: CommandItem; combo: string } | { kind: "pass" };

const PASS: KeyDecision = { kind: "pass" };

/** Commands that stay reachable while an overlay is open (they reopen or switch the box). */
function staysWithOverlay(id: string): boolean {
  return id.startsWith("search.") || id.startsWith("palette.") || id.startsWith("tab.");
}

/**
 * What a key press does. Passes (the browser and the widgets keep the key) when: another handler already took it,
 * it is part of a composition | dead key | AltGr, it is no combo of the grammar, it is a plain key while the person
 * types or an overlay is open, no command holds the combo, or none that holds it can run now. Several can run: the
 * narrower one (its scope inside the other's: in the Notes tab `notes.pinned1` over `tab.select1`), else the first
 * registered.
 */
export function resolveKey(
  event: KeyEventLike & { defaultPrevented?: boolean },
  env: KeyEnv,
): KeyDecision {
  if (event.defaultPrevented === true || env.overlay === "question") return PASS;
  const combo = comboFromEvent(event, env.mac);
  if (combo === null) return PASS;
  const boxed = env.overlay === "box";
  if (isSingleKey(combo) && (env.typing || boxed)) return PASS;
  let best: CommandItem | null = null;
  for (const id of commandsFor(env.keymap, combo)) {
    if (boxed && !staysWithOverlay(id)) continue;
    const command = env.commands.find((c) => c.id === id);
    if (!command || !runnable(command, env.context)) continue;
    if (best === null || narrower(scopeOf(command.requires), scopeOf(best.requires)))
      best = command;
  }
  return best === null ? PASS : { kind: "run", command: best, combo };
}

/** Whether keys typed now go into text (a field, a text area, an editor). */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return (
    target.closest(
      "input, textarea, select, [contenteditable=''], [contenteditable='true'], .cm-content",
    ) !== null
  );
}

/** Listens on `target` until the returned function is called. */
export function installKeys(
  target: Pick<Window, "addEventListener" | "removeEventListener">,
  env: (event: KeyboardEvent) => KeyEnv,
): () => void {
  const onKey = (event: Event): void => {
    if (!(event instanceof KeyboardEvent)) return;
    const decision = resolveKey(event, env(event));
    if (decision.kind !== "run") return;
    event.preventDefault();
    // a command reports its own failures (notices); a throw, now or later, must not break the listener
    try {
      void Promise.resolve(decision.command.run()).catch(() => {});
    } catch {
      // see above
    }
  };
  target.addEventListener("keydown", onKey);
  return () => target.removeEventListener("keydown", onKey);
}
