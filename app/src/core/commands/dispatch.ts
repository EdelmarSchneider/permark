// Key -> command. One `keydown` listener per window; the decision is a pure function so every guard is tested.

import type { Context } from "../conditions/available";
import { comboFromEvent, isSingleKey, type KeyEventLike } from "./combo";
import { runnable, type CommandItem } from "./commands";
import { commandsFor, type Keymap } from "./keymap";

export type KeyEnv = {
  mac: boolean;
  keymap: Keymap;
  /** Every registered command, in registration order. */
  commands: readonly CommandItem[];
  context: Context;
  /** An editable element has the focus: plain keys are text there. */
  typing: boolean;
  /** An overlay owns the keyboard (Search everywhere, the keyboard dialog). */
  modal: boolean;
};

export type KeyDecision = { kind: "run"; command: CommandItem; combo: string } | { kind: "pass" };

const PASS: KeyDecision = { kind: "pass" };

/** Commands that stay reachable while an overlay is open (they reopen or switch the box). */
function staysWithOverlay(id: string): boolean {
  return id.startsWith("search.") || id.startsWith("palette.") || id.startsWith("workspace.");
}

/**
 * What a key press does. Passes (the browser and the widgets keep the key) when: another handler already took it,
 * it is part of a composition | dead key | AltGr, it is no combo of the grammar, it is a plain key while the person
 * types or an overlay is open, no command holds the combo, or none that holds it can run now.
 */
export function resolveKey(
  event: KeyEventLike & { defaultPrevented?: boolean },
  env: KeyEnv,
): KeyDecision {
  if (event.defaultPrevented === true) return PASS;
  const combo = comboFromEvent(event, env.mac);
  if (combo === null) return PASS;
  if (isSingleKey(combo) && (env.typing || env.modal)) return PASS;
  for (const id of commandsFor(env.keymap, combo)) {
    if (env.modal && !staysWithOverlay(id)) continue;
    const command = env.commands.find((c) => c.id === id);
    if (command && runnable(command, env.context)) return { kind: "run", command, combo };
  }
  return PASS;
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
