// The command list: what the palette and the keyboard dialog show, from the registry, the context and the map.
// Pure.

import type { Availability, Context } from "../conditions/available";
import { available } from "../conditions/available";
import type { MessageKey } from "../i18n";
import { fuzzyMatch } from "../search/fuzzy";
import type { Contribution, ContributionOf } from "../plugins/registry.svelte";
import { comboLabel } from "./combo";
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

/** A combo as typed into the palette: lower case, no spaces. */
const typed = (text: string): string => text.toLowerCase().replace(/\s+/g, "");

/** The ways a person may type a combo: as Windows | Linux write it (`ctrl+alt+e`), as macOS does (`⌥⌘e`, `cmd+alt+e`),
 *  and in the map's own form (`mod+alt+e`). */
function typedForms(combo: string): string[] {
  return [
    comboLabel(combo, false),
    comboLabel(combo, true),
    combo,
    combo.replace("Mod", "Cmd"),
  ].map(typed);
}

/** A query that reads as keys: it has a `+`, or macOS key symbols (no match on keys: its words still count). */
const looksLikeKeys = (query: string): boolean => /\+|[⌘⌥⇧⌃]/.test(query);

/** The entries whose label matches the query, best first (all of them, in order, for an empty query). A query typed
 *  as keys (`ctrl+alt+e`, `ctrl+alt+`) also finds the commands on those keys, first. */
export function matchCommands(
  entries: readonly CommandEntry[],
  query: string,
  label: (key: MessageKey) => string,
): { entry: CommandEntry; positions: number[] }[] {
  if (query.trim() === "") return entries.map((entry) => ({ entry, positions: [] }));
  const keys = looksLikeKeys(query) ? typed(query) : null;
  const ranked: { entry: CommandEntry; score: number; positions: number[]; at: number }[] = [];
  entries.forEach((entry, at) => {
    const forms = keys === null ? [] : entry.combos.flatMap(typedForms);
    const onKeys =
      keys === null ? 0 : forms.includes(keys) ? 2 : forms.some((f) => f.startsWith(keys)) ? 1 : 0;
    if (onKeys > 0) {
      // above every label match: the person typed the keys
      ranked.push({ entry, score: Number.MAX_SAFE_INTEGER - 2 + onKeys, positions: [], at });
      return;
    }
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
