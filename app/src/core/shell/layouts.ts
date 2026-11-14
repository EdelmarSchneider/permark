// The arrangements the person saved under a name ("layouts"; app-wide, in the settings). Pure: what a name is, when
// two names are one layout, what was saved made safe, the changes of the list. A layout holds an arrangement and
// nothing else: the groups of the three regions, their order and shares (no sizes, no hidden | open | selected
// state: those are the project's). Every change answers the same list object when it changes nothing.

import { isRecord, readArrangement, type Arrangement } from "./arrangement";

export type Layout = { readonly name: string; readonly arrangement: Arrangement };

/** At most this many layouts are kept; a name has at most this many characters. The settings refuse a write past
 *  either bound. */
export const MAX_LAYOUTS = 20;
export const MAX_LAYOUT_NAME = 40;

/** What a name never holds: a control character, half of a character pair (neither can be written). */
const UNWRITABLE = /[\p{Cc}\p{Cs}]/u;

/**
 * A name as it is kept: composed (NFC), without the spaces around it, 1 to `MAX_LAYOUT_NAME` characters (code
 * points: an emoji is one). `null`: not a name.
 */
export function layoutName(raw: string): string | null {
  const name = raw.normalize("NFC").trim();
  const length = [...name].length;
  return length < 1 || length > MAX_LAYOUT_NAME || UNWRITABLE.test(name) ? null : name;
}

/** What two names of one layout have in common: the letters, whatever their case and however an accent is written. */
export function foldName(name: string): string {
  return name.normalize("NFC").toLowerCase();
}

/** The place of the layout whose name folds like `name` (-1: none). */
function placeOf(items: readonly Layout[], name: string): number {
  const folded = foldName(name);
  return items.findIndex((item) => foldName(item.name) === folded);
}

/** The layout of that name, in any spelling of it. */
export function findLayout(items: readonly Layout[], name: string): Layout | undefined {
  return items[placeOf(items, name)];
}

/**
 * The saved layouts, made safe: absent | damaged | another version -> none. An item without a usable name, with an
 * arrangement that cannot be read (`readArrangement`: the reader of the window's own), or named like one before it
 * is dropped; at most `MAX_LAYOUTS` are kept.
 */
export function parseLayouts(raw: unknown): Layout[] {
  if (!isRecord(raw) || raw["v"] !== 1 || !Array.isArray(raw["items"])) return [];
  const items: readonly unknown[] = raw["items"];
  const kept: Layout[] = [];
  for (const item of items) {
    if (kept.length >= MAX_LAYOUTS) break;
    if (!isRecord(item) || typeof item["name"] !== "string") continue;
    const name = layoutName(item["name"]);
    const arrangement = readArrangement(item["arrangement"]);
    if (name === null || arrangement === null || placeOf(kept, name) >= 0) continue;
    kept.push({ name, arrangement });
  }
  return kept;
}

/**
 * `arrangement` kept under `name`: a layout of that name (in any spelling) is replaced in place and takes the new
 * spelling; else it joins the end. No usable name | a new one in a full list: nothing changes.
 */
export function saveLayout(
  items: readonly Layout[],
  name: string,
  arrangement: Arrangement,
): readonly Layout[] {
  const kept = layoutName(name);
  if (kept === null) return items;
  const at = placeOf(items, kept);
  const old = items[at];
  if (old === undefined) {
    return items.length >= MAX_LAYOUTS ? items : [...items, { name: kept, arrangement }];
  }
  if (old.name === kept && old.arrangement === arrangement) return items;
  return items.map((item, i) => (i === at ? { name: kept, arrangement } : item));
}

/**
 * The layout `from` is called `to` from now on (its place and arrangement stay). Refused when `to` is another
 * layout's name in any spelling; another spelling of its own name is a rename like any other.
 */
export function renameLayout(
  items: readonly Layout[],
  from: string,
  to: string,
): readonly Layout[] {
  const kept = layoutName(to);
  const at = placeOf(items, from);
  const mine = items[at];
  if (kept === null || mine === undefined || mine.name === kept) return items;
  const taken = placeOf(items, kept);
  if (taken >= 0 && taken !== at) return items;
  return items.map((item, i) => (i === at ? { name: kept, arrangement: item.arrangement } : item));
}

/** The list without the layout of that name. */
export function deleteLayout(items: readonly Layout[], name: string): readonly Layout[] {
  const at = placeOf(items, name);
  return at < 0 ? items : items.filter((_, i) => i !== at);
}

/**
 * A layout taken out comes back at the place it had (`at`; past the end: the end). Its name taken meanwhile | a full
 * list: nothing changes.
 */
export function restoreLayout(
  items: readonly Layout[],
  layout: Layout,
  at: number,
): readonly Layout[] {
  if (items.length >= MAX_LAYOUTS || placeOf(items, layout.name) >= 0) return items;
  const place = Math.min(items.length, Math.max(0, at));
  return [...items.slice(0, place), layout, ...items.slice(place)];
}
