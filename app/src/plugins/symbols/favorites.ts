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
/** Longest group name (mirrors `settings::characters::MAX_GROUP_NAME_CHARS`). */
export const MAX_GROUP_NAME_CHARS = 48;

const VERSION = 1;

/** C0 | DEL | C1 controls, bidi embedding | override | isolate controls: inserted at the caret they hide or reorder
 *  the text around it (mirrors `settings::characters::is_unsafe_char`). */
const UNSAFE = /[\u0000-\u001f\u007f-\u009f‪-‮⁦-⁩]/u;
/** What Rust's `char::is_ascii_whitespace` accepts (no vertical tab). */
const ASCII_BLANK = /^[ \t\n\f\r]+$/;

/** A character the settings keep (mirrors `settings::valid_favorite_char`). */
function validChar(char: string): boolean {
  const length = [...char].length;
  return (
    length >= 1 &&
    length <= MAX_FAVORITE_CODE_POINTS &&
    !ASCII_BLANK.test(char) &&
    !UNSAFE.test(char)
  );
}

/** A group name the settings keep, as a groups file names a group (mirrors `settings::valid_favorite_group`). */
function validGroup(name: string): boolean {
  return (
    name !== "" &&
    name.trim() === name &&
    [...name].length <= MAX_GROUP_NAME_CHARS &&
    !/\p{Cc}/u.test(name)
  );
}

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
  if (!validChar(char) || chars.includes(char) || chars.length >= MAX_FAVORITE_CHARS) {
    return favorites;
  }
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
  if (!validGroup(name) || groups.includes(name) || groups.length >= MAX_FAVORITE_GROUPS) {
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
