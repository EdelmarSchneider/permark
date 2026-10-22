// Tags and descriptions as the UI treats them (pure): how a typed tag is cleaned (the same rules as the backend's
// `normalize`), the colors and icons a tag can wear, which tags to suggest, and how the tags view lists.

import type { TagCountView } from "../../api";
import type { MessageKey } from "../../core/i18n";

/** A tag is at most this many characters. */
export const MAX_TAG_CHARS = 40;
/** A description is at most this many characters. */
export const MAX_DESCRIPTION = 160;
/** A file has at most this many tags. */
export const MAX_TAGS = 100;

export {
  COLORS,
  COLOR_LABEL,
  ICONS,
  ICON_LABEL,
  isColor,
  isIcon,
  type TagColor,
  type TagIconName,
} from "../../core/looks/model";

/** The canonical form of a tag: trimmed, lowercased, whitespace runs turned into one `-`, cut to 40 characters. */
export function normalizeTag(raw: string): string | null {
  const joined = raw
    .split(/\s+/)
    .filter((w) => w !== "")
    .join("-")
    .toLowerCase();
  const tag = [...joined].slice(0, MAX_TAG_CHARS).join("");
  return tag === "" ? null : tag;
}

/** Every tag normalized, empty ones and repeats dropped (the first spelling's place wins). */
export function normalizeTags(raw: readonly string[]): string[] {
  const out: string[] = [];
  for (const item of raw) {
    const tag = normalizeTag(item);
    if (tag !== null && !out.includes(tag) && out.length < MAX_TAGS) out.push(tag);
  }
  return out;
}

/** A description on one line, cut to its limit. */
export function normalizeDescription(raw: string): string {
  return [
    ...raw
      .split(/\s+/)
      .filter((w) => w !== "")
      .join(" "),
  ]
    .slice(0, MAX_DESCRIPTION)
    .join("");
}

/** What was typed in the tag field: a comma ends a tag, so `a, b` is two. */
export function splitTyped(text: string): { done: string[]; rest: string } {
  const parts = text.split(",");
  const rest = parts.pop() ?? "";
  const done = parts.map(normalizeTag).filter((t): t is string => t !== null);
  return { done, rest };
}

/** The tags to suggest for what was typed: not already on the file, names that start with it first, then the ones
 *  that contain it; most used first within each. An empty field suggests the most used. */
export function suggest(
  counts: readonly TagCountView[],
  typed: string,
  have: readonly string[],
  limit = 8,
): TagCountView[] {
  const needle = (normalizeTag(typed) ?? "").toLowerCase();
  const free = counts.filter((c) => !have.includes(c.name));
  if (needle === "") return free.slice(0, limit);
  const starts = free.filter((c) => c.name.startsWith(needle));
  const contains = free.filter((c) => !c.name.startsWith(needle) && c.name.includes(needle));
  return [...starts, ...contains].slice(0, limit);
}

/** The tags the tags view lists for the filter box (names that contain the words, in the order given). */
export function filterCounts(counts: readonly TagCountView[], query: string): TagCountView[] {
  const words = query
    .toLowerCase()
    .split(/\s+/)
    .filter((w) => w !== "");
  if (words.length === 0) return [...counts];
  return counts.filter((c) => words.every((w) => c.name.includes(w)));
}

/** The tooltip of a row: the tags and the description. */
export function tipOf(tags: readonly string[], description: string, tagsLabel: string): string {
  const lines: string[] = [];
  if (tags.length > 0) lines.push(`${tagsLabel}: ${tags.join(", ")}`);
  if (description !== "") lines.push(description);
  return lines.join("\n");
}
