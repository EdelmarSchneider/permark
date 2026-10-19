// The keys of a command as a button shows them ("Ctrl+O"), from the effective keyboard map; `null` when it has none.

import { conditions } from "../conditions/context.svelte";
import { comboLabel } from "./combo";
import { comboOf } from "./keymap";
import { keys } from "./state.svelte";

export function keyHint(command: string): string | null {
  const combo = comboOf(keys.map, command);
  return combo === null || combo === ""
    ? null
    : comboLabel(combo, conditions.current.platform === "mac");
}
