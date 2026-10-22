// The window's keyboard and mouse side buttons: the core's commands go into the registry and one listener per input
// runs them. One call per window.

import { conditions } from "../conditions/context.svelte";
import { contributions } from "../plugins/instance.svelte";
import { ui } from "../shell/ui.svelte";
import { registerCoreCommands } from "./core";
import { installButtons, installKeys, isTypingTarget } from "./dispatch";
import { keys } from "./state.svelte";

/** Registers the core's commands and starts listening on `target`; returns the teardown of all three. */
export function installWindowKeys(
  target: Pick<Window, "addEventListener" | "removeEventListener"> = window,
): () => void {
  const unregister = registerCoreCommands(contributions);
  const stop = installKeys(target, (event) => ({
    mac: conditions.current.platform === "mac",
    keymap: keys.map,
    commands: contributions.of("command"),
    context: conditions.current,
    typing: isTypingTarget(event.target),
    overlay: ui.overlay,
  }));
  const stopButtons = installButtons(target, () => ({
    commands: contributions.of("command"),
    context: conditions.current,
    overlay: ui.overlay,
    dialog: document.querySelector('[aria-modal="true"]') !== null,
  }));
  return () => {
    stopButtons();
    stop();
    unregister();
  };
}
