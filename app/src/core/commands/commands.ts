// The command list: what the palette and the keyboard dialog show, from the registry, the context and the map.
// Pure.

import type { Availability, Context } from "../conditions/available";
import { available } from "../conditions/available";
import type { MessageKey } from "../i18n";
import { fuzzyMatch } from "../search/fuzzy";
import type { Contribution, ContributionOf } from "../plugins/registry.svelte";
import { comboOf, type Keymap } from "./keymap";

export type CommandItem = ContributionOf<"command">;

export type CommandEntry = {
  command: CommandItem;
  /** Effective combos (the person's map over the defaults). */
  combos: readonly string[];
  availability: Availability;
};

/**
 * The commands to offer now, in registration order: `when` false hides a command; `palette` also hides the ones that
 * open the box themselves; `all` lists every command (the keyboard dialog). Unavailable commands stay (the palette
 * dims them and says why).
 */
export function listCommands(
  items: readonly Contribution[],
  context: Context,
  keymap: Keymap,
  options: { palette: boolean; all?: boolean },
): CommandEntry[] {
  const out: CommandEntry[] = [];
  for (const item of items) {
    if (item.kind !== "command") continue;
    if (options.all !== true) {
      if (options.palette && item.inPalette === false) continue;
      if (item.when && !item.when()) continue;
    }
    out.push({
      command: item,
      combos: keymap.combos.get(item.id) ?? [],
      availability: available(item.requires, context),
    });
  }
  return out;
}

/** The entries whose label matches the query, best first (all of them, in order, for an empty query). */
export function matchCommands(
  entries: readonly CommandEntry[],
  query: string,
  label: (key: MessageKey) => string,
): { entry: CommandEntry; positions: number[] }[] {
  if (query.trim() === "") return entries.map((entry) => ({ entry, positions: [] }));
  const ranked: { entry: CommandEntry; score: number; positions: number[]; at: number }[] = [];
  entries.forEach((entry, at) => {
    const found = fuzzyMatch(query, label(entry.command.label));
    if (found) ranked.push({ entry, score: found.score, positions: found.positions, at });
  });
  ranked.sort((a, b) => b.score - a.score || a.at - b.at);
  return ranked.map(({ entry, positions }) => ({ entry, positions }));
}

/** The first combo of a command in the map (what a row shows). */
export function shortcutOf(keymap: Keymap, command: string): string | null {
  return comboOf(keymap, command);
}

/** Whether a command may run now: `when` true and `available` not `unavailable`. */
export function runnable(command: CommandItem, context: Context): boolean {
  if (command.when && !command.when()) return false;
  return available(command.requires, context).status !== "unavailable";
}
