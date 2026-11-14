// The person's favorite characters and pinned groups, changed one step at a time, pure. Order = the order they were
// added (no reorder: remove, then add again). Characters compare by their exact code points, never normalized: "❤"
// and "❤️" are two characters, and so are the Ohm sign U+2126 and the Greek capital omega U+03A9. A step that
// changes nothing (a repeat add, a remove of what is not there) or that the settings would refuse hands back the
// very value it got, so the caller knows there is nothing to write. Both lists empty -> `null`: the key goes away.

import type { SymbolFavorites } from "../../api";

/** Most favorite characters kept (mirrors the backend's `settings::MAX_FAVORITE_CHARS`). */
export const MAX_FAVORITE_CHARS = 64;
/** Most pinned groups kept (mirrors the backend's `settings::MAX_FAVORITE_GROUPS`). */
export const MAX_FAVORITE_GROUPS = 16;
/** Longest favorite character in code points (mirrors `settings::characters::MAX_ENTRY_CHARS`). */
export const MAX_FAVORITE_CODE_POINTS = 16;

const VERSION = 1;

export function hasChar(favorites: SymbolFavorites | null, char: string): boolean {
  return favorites?.chars.includes(char) ?? false;
}

export function hasGroup(favorites: SymbolFavorites | null, name: string): boolean {
  return favorites?.groups.includes(name) ?? false;
}

/** The value with `chars` | `groups` replaced; `null` when both end empty. */
function next(
  favorites: SymbolFavorites | null,
  chars: readonly string[],
  groups: readonly string[],
): SymbolFavorites | null {
  if (chars.length === 0 && groups.length === 0) return null;
  return { v: favorites?.v ?? VERSION, chars: [...chars], groups: [...groups] };
}

export function withChar(favorites: SymbolFavorites | null, char: string): SymbolFavorites | null {
  const chars = favorites?.chars ?? [];
  const length = [...char].length;
  if (length === 0 || length > MAX_FAVORITE_CODE_POINTS) return favorites;
  if (chars.includes(char) || chars.length >= MAX_FAVORITE_CHARS) return favorites;
  return next(favorites, [...chars, char], favorites?.groups ?? []);
}

export function withoutChar(
  favorites: SymbolFavorites | null,
  char: string,
): SymbolFavorites | null {
  if (favorites === null || !favorites.chars.includes(char)) return favorites;
  return next(
    favorites,
    favorites.chars.filter((c) => c !== char),
    favorites.groups,
  );
}

export function withGroup(favorites: SymbolFavorites | null, name: string): SymbolFavorites | null {
  const groups = favorites?.groups ?? [];
  if (name === "" || groups.includes(name) || groups.length >= MAX_FAVORITE_GROUPS) {
    return favorites;
  }
  return next(favorites, favorites?.chars ?? [], [...groups, name]);
}

export function withoutGroup(
  favorites: SymbolFavorites | null,
  name: string,
): SymbolFavorites | null {
  if (favorites === null || !favorites.groups.includes(name)) return favorites;
  return next(
    favorites,
    favorites.chars,
    favorites.groups.filter((g) => g !== name),
  );
}
