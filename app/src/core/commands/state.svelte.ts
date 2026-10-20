// The effective keyboard map of this window: the default keys the registry holds, the person's map over them.

import { conditions } from "../conditions/context.svelte";
import { contributions } from "../plugins/instance.svelte";
import { windowStore } from "../window.svelte";
import { buildKeymap, type DefaultKey, type Keymap } from "./keymap";

class KeymapState {
  /** Default keys + the person's overrides; recomputed when either changes. */
  map: Keymap = $derived.by(() => {
    const defaults: DefaultKey[] = [];
    const commands: string[] = [];
    for (const item of contributions.items) {
      if (item.kind === "shortcut") defaults.push({ command: item.command, keys: item.keys });
      else if (item.kind === "command") commands.push(item.id);
    }
    return buildKeymap(
      defaults,
      windowStore.keymap,
      commands,
      conditions.current.platform === "mac",
    );
  });
}

export const keys = new KeymapState();
