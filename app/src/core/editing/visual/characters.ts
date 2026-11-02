// The model behind the emoji + symbol pickers: the groups and entries the settings payload ships, scanned in
// payload order and filtered by name. The groups file is the source (`settings/characters.rs` ships it) and the
// matching is the app's own `core/search/fuzzy.ts` — no character table, no matcher of our own.

import type { CharacterEntry, CharacterGroup } from "../../../api";
import { fuzzyMatch } from "../../search/fuzzy";

/** Which picker is open: the payload's emoji group, or the symbol groups. */
export type PickerKind = "emoji" | "symbols";

/** One group as a picker renders it: the payload's name, and the entries it shows. */
export type PickerGroup = { name: string; entries: CharacterEntry[] };

/** Entries one group shows when nothing is typed (a group can ship a thousand). */
export const GROUP_RENDER_CAP = 60;

/** Entries a query shows, over all the groups of the kind. */
export const QUERY_RENDER_CAP = 200;

/** The payload's group of emoji is named this (the name `defaults()` of `settings/characters.rs` uses). */
const EMOJI_GROUP = "emoji";

/**
 * The tier of a score: `fuzzyMatch` scores by how the query matched (whole-name prefix, a name that contains
 * it, a subsequence), and the tiers are what a picker ranks by — inside a tier the payload's own order holds,
 * so a search never reorders what the groups file lists.
 */
function tierOf(score: number): number {
  return Math.trunc(score / 100);
}

/** The best tier an entry reaches over its names; `null` when the query answers none of them. */
function bestTier(query: string, entry: CharacterEntry): number | null {
  let best: number | null = null;
  for (const name of entry.names) {
    const match = fuzzyMatch(query, name);
    if (!match) continue;
    const tier = tierOf(match.score);
    if (best === null || tier > best) best = tier;
  }
  return best;
}

/** The emoji kind is the payload's `emoji` group; the symbols kind is every other group, in payload order. */
export function groupsFor(kind: PickerKind, groups: readonly CharacterGroup[]): CharacterGroup[] {
  const isEmoji = (group: CharacterGroup): boolean =>
    group.name.trim().toLowerCase() === EMOJI_GROUP;
  return kind === "emoji"
    ? groups.filter((group) => isEmoji(group))
    : groups.filter((group) => !isEmoji(group));
}

/**
 * What a picker shows: on an empty (or blank) query, the groups of the kind in payload order, each sliced to
 * `GROUP_RENDER_CAP`; on a query, the entries whose names answer it — the better-named first inside a group,
 * the payload's order breaking the ties — the groups left with nothing dropped, stopping at `QUERY_RENDER_CAP`.
 */
export function scanGroups(
  kind: PickerKind,
  groups: readonly CharacterGroup[],
  query: string,
): PickerGroup[] {
  const picked = groupsFor(kind, groups);
  if (query.trim() === "") {
    return picked.map((group) => ({
      name: group.name,
      entries: group.entries.slice(0, GROUP_RENDER_CAP),
    }));
  }
  const shown: PickerGroup[] = [];
  let room = QUERY_RENDER_CAP;
  for (const group of picked) {
    if (room <= 0) break;
    const hits: { entry: CharacterEntry; tier: number; at: number }[] = [];
    group.entries.forEach((entry, at) => {
      const tier = bestTier(query, entry);
      if (tier !== null) hits.push({ entry, tier, at });
    });
    if (hits.length === 0) continue;
    hits.sort((a, b) => b.tier - a.tier || a.at - b.at);
    const entries = hits.slice(0, room).map((hit) => hit.entry);
    room -= entries.length;
    shown.push({ name: group.name, entries });
  }
  return shown;
}
