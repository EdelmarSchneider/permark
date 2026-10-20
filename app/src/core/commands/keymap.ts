// The keyboard map: the defaults the `shortcut` contributions give, the person's overrides, the combos that result
// and the commands that fight over one. Pure.

import type { Platform } from "../../api/generated/Platform";
import type { WorkspaceKind } from "../../api/generated/WorkspaceKind";
import type { Requirement } from "../conditions/available";
import { normalizeCombo } from "./combo";

/** A default key of a command (a `shortcut` contribution). */
export type DefaultKey = { command: string; keys: string };

/**
 * `combos`: command -> its combos, in the order the defaults were registered; `owners`: combo -> commands; `custom`:
 * commands holding the person's own combo (not their defaults).
 */
export type Keymap = {
  combos: ReadonlyMap<string, readonly string[]>;
  owners: ReadonlyMap<string, readonly string[]>;
  custom: ReadonlySet<string>;
};

/** A combo two or more commands hold. */
export type Conflict = { combo: string; commands: readonly string[] };

/**
 * The effective map (combos as `mac` spells them: `Ctrl` is `Mod` elsewhere). A command with an override has exactly
 * that combo (`""` = unbound); an override that is not a combo is ignored (the defaults stay). Commands appear in the order of `commands` (registration order), then those
 * only the overrides name.
 */
export function buildKeymap(
  defaults: readonly DefaultKey[],
  overrides: Readonly<Record<string, string>>,
  commands: readonly string[],
  mac = false,
): Keymap {
  const byDefault = new Map<string, string[]>();
  for (const { command, keys } of defaults) {
    const combo = normalizeCombo(keys, mac);
    if (combo === null) continue;
    const list = byDefault.get(command) ?? [];
    if (!list.includes(combo)) list.push(combo);
    byDefault.set(command, list);
  }
  const combos = new Map<string, string[]>();
  const custom = new Set<string>();
  const order = [...new Set([...commands, ...Object.keys(overrides)])];
  for (const command of order) {
    const override = overrides[command];
    let list: string[];
    if (override === undefined) list = byDefault.get(command) ?? [];
    else if (override === "") list = [];
    else {
      const combo = normalizeCombo(override, mac);
      list = combo === null ? (byDefault.get(command) ?? []) : [combo];
      if (combo !== null) custom.add(command);
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
  return { combos, owners, custom };
}

/** What a command's requirement lets it run in, as far as keys go. `when()` is not part of it. */
export type Scope = {
  kinds: ReadonlySet<WorkspaceKind>;
  platforms: ReadonlySet<Platform>;
};

/** The requirement of a command by id (`undefined`: none known, it may run anywhere). */
export type RequiresOf = (command: string) => Requirement | undefined;

// every kind | platform (the compiler keeps the lists complete)
const ALL_KINDS = Object.keys({
  welcome: true,
  project: true,
  singleFile: true,
  notes: true,
} satisfies Record<WorkspaceKind, true>) as WorkspaceKind[];
const ALL_PLATFORMS = Object.keys({
  windows: true,
  mac: true,
  linux: true,
} satisfies Record<Platform, true>) as Platform[];

/** The workspace kinds + platforms a requirement allows (several needs of one sort: what they all allow). */
export function scopeOf(requires: Requirement | undefined): Scope {
  let kinds = new Set(ALL_KINDS);
  let platforms = new Set(ALL_PLATFORMS);
  for (const need of requires?.needs ?? []) {
    if (need.need === "workspace") kinds = new Set(need.kinds.filter((k) => kinds.has(k)));
    else if (need.need === "platform") {
      platforms = new Set(need.platforms.filter((p) => platforms.has(p)));
    }
  }
  return { kinds, platforms };
}

function overlaps<T>(a: ReadonlySet<T>, b: ReadonlySet<T>): boolean {
  for (const v of a) if (b.has(v)) return true;
  return false;
}

function within<T>(a: ReadonlySet<T>, b: ReadonlySet<T>): boolean {
  for (const v of a) if (!b.has(v)) return false;
  return true;
}

/** `a` lies strictly inside `b`: wherever `a` can run, `b` can too, not the other way round (`a` wins its keys). */
export function narrower(a: Scope, b: Scope): boolean {
  const inside = within(a.kinds, b.kinds) && within(a.platforms, b.platforms);
  const back = within(b.kinds, a.kinds) && within(b.platforms, a.platforms);
  return inside && !back;
}

/**
 * Two commands on one combo fight: both can run in the same place. Both on their defaults (`designed`): a narrower
 * one takes the key in its place on purpose (the Notes tab's own keys), so only scopes that overlap with neither
 * inside the other fight. A person's own combo fights any command that can run where it can (they may not know that
 * the narrower one wins there).
 */
export function clash(a: Scope, b: Scope, designed = true): boolean {
  const together = overlaps(a.kinds, b.kinds) && overlaps(a.platforms, b.platforms);
  return together && (!designed || (!narrower(a, b) && !narrower(b, a)));
}

/** The combos two commands hold that fight over them (`clash`); only those commands listed, in map order. */
export function conflicts(map: Keymap, requiresOf: RequiresOf): Conflict[] {
  const found: Conflict[] = [];
  for (const [combo, commands] of map.owners) {
    if (commands.length < 2) continue;
    const fighting = commands.filter((c) => clashesWith(map, commands, c, requiresOf).length > 0);
    if (fighting.length > 0) found.push({ combo, commands: fighting });
  }
  return found;
}

function clashesWith(
  map: Keymap,
  commands: readonly string[],
  command: string,
  requiresOf: RequiresOf,
): string[] {
  const own = scopeOf(requiresOf(command));
  return commands.filter(
    (c) =>
      c !== command &&
      clash(own, scopeOf(requiresOf(c)), !map.custom.has(command) && !map.custom.has(c)),
  );
}

/** The other commands that fight `command` over `combo` (not `command` itself). */
export function heldBy(
  map: Keymap,
  combo: string,
  command: string,
  requiresOf: RequiresOf,
): readonly string[] {
  return clashesWith(map, map.owners.get(combo) ?? [], command, requiresOf);
}

/** The commands a combo runs, in registration order. */
export function commandsFor(map: Keymap, combo: string): readonly string[] {
  return map.owners.get(combo) ?? [];
}

/** The first combo of a command (the one the palette shows). */
export function comboOf(map: Keymap, command: string): string | null {
  return map.combos.get(command)?.[0] ?? null;
}
