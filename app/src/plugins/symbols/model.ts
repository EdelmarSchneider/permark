// What the symbols panel draws, pure: the person's favorite characters on top, the groups they pinned next (in the
// groups file's order, not repeated below), then the other groups (the symbol groups, then the emoji group). The
// groups and the matcher are the pickers' own (`core/editing/visual/characters.ts`); characters are compared by their
// exact code points, never normalized (a heart with the emoji selector is another character than one without).

import type { CharacterEntry, CharacterGroup, SymbolFavorites } from "../../api";
import type { EditMode } from "../../core/editing/session.svelte";
import {
  groupsFor,
  isBlankQuery,
  matchEntries,
  scanGroups,
  type PickerGroup,
} from "../../core/editing/visual/characters";

/** The loose favorite characters, one grid without a disclosure. */
export type FavoritesSection = {
  kind: "favorites";
  key: string;
  entries: readonly CharacterEntry[];
};

/** One group: `entries` are what it draws now (none while collapsed), `total` what its header counts. */
export type GroupSection = {
  kind: "group";
  key: string;
  name: string;
  entries: readonly CharacterEntry[];
  total: number;
  expanded: boolean;
  /** One of the groups the person keeps on top. */
  pinned: boolean;
};

/** The line between the pinned groups and the others. */
export type DividerSection = { kind: "divider"; key: string };

export type Section = FavoritesSection | GroupSection | DividerSection;

/** The groups a fresh panel shows collapsed (by section key): the emoji group of the built-ins, it is long. */
export const DEFAULT_COLLAPSED: ReadonlySet<string> = new Set(["emoji"]);

/** Keys no group name can take (group keys are their names; a name never holds U+0000). */
const FAVORITES_KEY = "\u0000favorites";
const DIVIDER_KEY = "\u0000divider";

/** The key of each group, in order: its name, a repeated name made distinct (keyed each blocks need that). */
function keysOf(groups: readonly PickerGroup[]): string[] {
  const seen = new Map<string, number>();
  return groups.map((group) => {
    const n = seen.get(group.name) ?? 0;
    seen.set(group.name, n + 1);
    return n === 0 ? group.name : `${group.name}\u0000${n}`;
  });
}

/** The entry a favorite character draws: the groups' own (its names), else the bare character. */
function favoriteEntry(char: string, groups: readonly CharacterGroup[]): CharacterEntry {
  for (const group of groups) {
    const found = group.entries.find((entry) => entry.char === char);
    if (found !== undefined) return found;
  }
  return { char, names: [] };
}

/**
 * The sections of the panel. Empty query: the favorites section always (its hint when empty), every group, an
 * expanded one with all its entries (no cap), a collapsed one (`collapsed`, by section key) with none. A query: the
 * pickers' matches over the symbol groups and the emoji group (their caps apply), every matching group expanded,
 * the favorites that answer it (the section left out when none does).
 */
export function sectionsOf(
  groups: readonly CharacterGroup[],
  favorites: SymbolFavorites | null,
  query: string,
  collapsed: ReadonlySet<string>,
): Section[] {
  const querying = !isBlankQuery(query);
  const loose = (favorites?.chars ?? []).map((char) => favoriteEntry(char, groups));
  const favored = matchEntries(query, loose);
  const pinnedNames = new Set(favorites?.groups ?? []);
  // the symbol groups, then the emoji group; a query: the pickers' matches, in the same order
  const shown: PickerGroup[] = querying
    ? [...scanGroups("symbols", groups, query), ...scanGroups("emoji", groups, query)]
    : [...groupsFor("symbols", groups), ...groupsFor("emoji", groups)];
  const keys = keysOf(shown);
  const sections: GroupSection[] = shown.map((group, at) => {
    const key = keys[at] ?? group.name;
    const expanded = querying || !collapsed.has(key);
    return {
      kind: "group",
      key,
      name: group.name,
      entries: expanded ? group.entries : [],
      total: group.entries.length,
      expanded,
      pinned: pinnedNames.has(group.name),
    };
  });
  // pinned groups keep the groups file's order (the emoji group included where the file lists it)
  const fileOrder = new Map(groups.map((group, at) => [group.name, at] as const).reverse());
  const rank = (section: GroupSection): number => fileOrder.get(section.name) ?? 0;
  const pinned = sections.filter((s) => s.pinned).sort((a, b) => rank(a) - rank(b));
  const others = sections.filter((s) => !s.pinned);
  return [
    ...(!querying || favored.length > 0
      ? [{ kind: "favorites", key: FAVORITES_KEY, entries: favored } satisfies FavoritesSection]
      : []),
    ...pinned,
    ...(pinned.length > 0 && others.length > 0
      ? [{ kind: "divider", key: DIVIDER_KEY } satisfies DividerSection]
      : []),
    ...others,
  ];
}

/** Whether a click inserts: an editor (Edit | Source) on the shown document, else why not. An editor that still
 *  refuses at the caret (a sheet, a block kept as written) answers `false` to the insert itself. */
export function canInsert(mode: EditMode, docOpen: boolean): "ok" | "view" | "none" {
  if (!docOpen) return "none";
  return mode === "view" ? "view" : "ok";
}
