// The model behind the emoji + symbol pickers: the groups and entries the settings payload ships, scanned in
// payload order and filtered by name, by the character itself or by its code point. The groups file is the source
// (`settings/characters.rs` ships it) and name matching is the app's own `core/search/fuzzy.ts` — no character
// table, no matcher of our own. Also what a cell draws and says, for every view that lists the groups.

import type { CharacterEntry, CharacterGroup } from "../../../api";
import { t, type MessageKey } from "../../i18n";
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

/** The groups the built-in file names get a label in the person's language; any other name shows as it is. */
const GROUP_KEYS: Readonly<Record<string, MessageKey>> = {
  emoji: "picker.group.emoji",
  greek: "picker.group.greek",
  operators: "picker.group.operators",
  relations: "picker.group.relations",
  arrows: "picker.group.arrows",
  units: "picker.group.units",
  scripts: "picker.group.scripts",
  spaces: "picker.group.spaces",
};

/** The label of a group: the translated one for a built-in name, else the name the groups file gives. */
export function groupLabel(name: string): string {
  const folded = name.trim().toLowerCase();
  // own keys only: a group named "constructor" never reads Object.prototype
  const key = Object.hasOwn(GROUP_KEYS, folded) ? GROUP_KEYS[folded] : undefined;
  return key === undefined ? name : t(key);
}

/**
 * The tier of a score: `fuzzyMatch` scores by how the query matched (whole-name prefix, a name that contains
 * it, a subsequence), and the tiers are what a picker ranks by — inside a tier the payload's own order holds,
 * so a search never reorders what the groups file lists.
 */
function tierOf(score: number): number {
  return Math.trunc(score / 100);
}

/** An entry that is the pasted character itself; above every name tier `fuzzyMatch` can reach. */
const CHAR_TIER = 101;
/** An entry that is (or starts with) the code point the query writes. */
const CODE_POINT_TIER = 100;

/** ASCII whitespace around a query; `String.trim` would also drop a pasted no-break or thin space. */
const ASCII_EDGES = /^[ \t\r\n]+|[ \t\r\n]+$/g;

/** "U+2192", "u+2192" or "2192": 4 to 6 hex digits, as character tables write a code point. */
const CODE_POINT_QUERY = /^(?:u\+)?([0-9a-f]{4,6})$/i;

/** What a query asks besides names: the character itself, and the code point it writes (`null`: none). */
type Asked = { text: string; codePoint: number | null };

function asked(query: string): Asked {
  const text = query.replace(ASCII_EDGES, "");
  const hex = CODE_POINT_QUERY.exec(text)?.[1];
  const codePoint = hex === undefined ? null : Number.parseInt(hex, 16);
  return { text, codePoint: codePoint !== null && codePoint <= 0x10ffff ? codePoint : null };
}

/** The best tier an entry reaches: the pasted character, its code point, or its names; `null`: none answers. */
function bestTier(query: Asked, entry: CharacterEntry): number | null {
  if (entry.char === query.text) return CHAR_TIER;
  if (query.codePoint !== null && entry.char.codePointAt(0) === query.codePoint) {
    return CODE_POINT_TIER;
  }
  let best: number | null = null;
  for (const name of entry.names) {
    const match = fuzzyMatch(query.text, name);
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
 * What a picker shows: on an empty (or ASCII-blank) query, the groups of the kind in payload order, each sliced
 * to `GROUP_RENDER_CAP`; on a query, the entries that are the pasted character, start with the code point it
 * writes ("U+2192" | "2192") or whose names answer it — in that order inside a group, the payload's order
 * breaking the ties — the groups left with nothing dropped, stopping at `QUERY_RENDER_CAP`.
 */
export function scanGroups(
  kind: PickerKind,
  groups: readonly CharacterGroup[],
  query: string,
): PickerGroup[] {
  const picked = groupsFor(kind, groups);
  const ask = asked(query);
  if (ask.text === "") {
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
      const tier = bestTier(ask, entry);
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

/** A space or a format character (no-break, thin, word joiner): nothing to see when drawn. */
const INVISIBLE = /^[\p{White_Space}\p{Cf}]$/u;

/** Letters an invisible entry's cell shows of its name. */
const LABEL_LENGTH = 5;

/** Code points a cell's tooltip lists before it cuts a long sequence. */
const SHOWN_CODE_POINTS = 3;

/** "U+2192" for one code point (at least four hex digits, as character tables write them). */
function codePointText(char: string): string {
  return `U+${(char.codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, "0")}`;
}

/** Whether every code point of `char` is a space or a format character, so a cell drawing it would look empty. */
export function isInvisible(char: string): boolean {
  const points = [...char];
  return points.length > 0 && points.every((point) => INVISIBLE.test(point));
}

/** The code points of `char` ("U+03B1"); a longer sequence (family emoji) shows its first three, then "…". */
export function codePoints(char: string): string {
  const points = [...char].map(codePointText);
  const shown = points.slice(0, SHOWN_CODE_POINTS).join(" ");
  return points.length > SHOWN_CODE_POINTS ? `${shown} …` : shown;
}

/** What an entry is called: its first name, else every code point it is made of. */
export function entryName(entry: CharacterEntry): string {
  return entry.names[0] ?? [...entry.char].map(codePointText).join(" ");
}

/**
 * What a cell draws: the character itself, or for an invisible one its shortest name in capitals ("NBSP", "WJ"),
 * else its code point's hex digits, cut to a few letters so it fits the cell.
 */
export function cellLabel(entry: CharacterEntry): string {
  if (!isInvisible(entry.char)) return entry.char;
  const shortest = entry.names.reduce<string | null>(
    (best, name) => (best === null || name.length < best.length ? name : best),
    null,
  );
  const label = shortest?.toUpperCase() ?? codePointText(entry.char).slice("U+".length);
  return label.slice(0, LABEL_LENGTH).trimEnd();
}
