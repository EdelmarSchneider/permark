// The keyboard map: the defaults the `shortcut` contributions give, the person's overrides, the combos that result
// and the commands that fight over one. Pure.

import { normalizeCombo } from "./combo";

/** A default key of a command (a `shortcut` contribution). */
export type DefaultKey = { command: string; keys: string };

/** `combos`: command -> its combos, in the order the defaults were registered; `owners`: combo -> commands. */
export type Keymap = {
  combos: ReadonlyMap<string, readonly string[]>;
  owners: ReadonlyMap<string, readonly string[]>;
};

/** A combo two or more commands hold. */
export type Conflict = { combo: string; commands: readonly string[] };

/**
 * The effective map. A command with an override has exactly that combo (`""` = unbound); an override that is not a
 * combo is ignored (the defaults stay). Commands appear in the order of `commands` (registration order), then those
 * only the overrides name.
 */
export function buildKeymap(
  defaults: readonly DefaultKey[],
  overrides: Readonly<Record<string, string>>,
  commands: readonly string[],
): Keymap {
  const byDefault = new Map<string, string[]>();
  for (const { command, keys } of defaults) {
    const combo = normalizeCombo(keys);
    if (combo === null) continue;
    const list = byDefault.get(command) ?? [];
    if (!list.includes(combo)) list.push(combo);
    byDefault.set(command, list);
  }
  const combos = new Map<string, string[]>();
  const order = [...new Set([...commands, ...Object.keys(overrides)])];
  for (const command of order) {
    const override = overrides[command];
    let list: string[];
    if (override === undefined) list = byDefault.get(command) ?? [];
    else if (override === "") list = [];
    else {
      const combo = normalizeCombo(override);
      list = combo === null ? (byDefault.get(command) ?? []) : [combo];
    }
    if (list.length > 0) combos.set(command, list);
  }
  const owners = new Map<string, string[]>();
  for (const [command, list] of combos) {
    for (const combo of list) {
      const held = owners.get(combo) ?? [];
      held.push(command);
      owners.set(combo, held);
    }
  }
  return { combos, owners };
}

/** The combos held by more than one command. */
export function conflicts(map: Keymap): Conflict[] {
  const found: Conflict[] = [];
  for (const [combo, commands] of map.owners) {
    if (commands.length > 1) found.push({ combo, commands });
  }
  return found;
}

/** The other commands that hold `combo` (not `command` itself). */
export function heldBy(map: Keymap, combo: string, command: string): readonly string[] {
  return (map.owners.get(combo) ?? []).filter((c) => c !== command);
}

/** The commands a combo runs, in registration order. */
export function commandsFor(map: Keymap, combo: string): readonly string[] {
  return map.owners.get(combo) ?? [];
}

/** The first combo of a command (the one the palette shows). */
export function comboOf(map: Keymap, command: string): string | null {
  return map.combos.get(command)?.[0] ?? null;
}
