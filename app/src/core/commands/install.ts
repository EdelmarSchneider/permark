// The window's keyboard: the core's commands go into the registry and one listener runs them. One call per window.

import { conditions } from "../conditions/context.svelte";
import { contributions } from "../plugins/instance.svelte";
import { ui } from "../shell/ui.svelte";
import { registerCoreCommands } from "./core";
import { installKeys, isTypingTarget } from "./dispatch";
import { keys } from "./state.svelte";

/** Registers the core's commands and starts listening on `target`; returns the teardown of both. */
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
    modal: ui.modal,
  }));
  return () => {
    stop();
    unregister();
  };
}
