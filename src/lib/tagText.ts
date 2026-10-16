// How tags and descriptions are cleaned up. The same rules as the backend's `meta::normalize`, so what
// the UI shows while typing is what gets stored.

export const MAX_TAG_CHARS = 40;
export const MAX_DESCRIPTION_CHARS = 120;

const cut = (text: string, max: number): string => Array.from(text).slice(0, max).join("");

/** Trimmed, lowercased, runs of whitespace turned into one "-", cut to 40 characters. Null when empty. */
export function normalizeTag(raw: string): string | null {
  const tag = cut(raw.trim().split(/\s+/).join("-").toLowerCase(), MAX_TAG_CHARS);
  return tag === "" ? null : tag;
}

/** Normalizes every tag, dropping empty ones and repeats (the first one keeps its place). */
export function normalizeTags(raw: readonly string[]): string[] {
  const out: string[] = [];
  for (const tag of raw) {
    const clean = normalizeTag(tag);
    if (clean !== null && !out.includes(clean)) out.push(clean);
  }
  return out;
}

/** One line: whitespace runs (newlines included) become one space; trimmed; cut to 120 characters. */
export function normalizeDescription(raw: string): string {
  return cut(raw.trim().split(/\s+/).join(" "), MAX_DESCRIPTION_CHARS);
}

/**
 * Typing in the tag box: everything before a comma is a finished tag, what follows the last comma is
 * still being typed.
 */
export function splitTagInput(text: string): { done: string[]; rest: string } {
  const parts = text.split(",");
  const rest = parts.pop() ?? "";
  return { done: normalizeTags(parts), rest };
}

/**
 * The tags still waiting in the box when the editor closes: whatever was typed and not yet confirmed with
 * Enter or a comma. Nothing the reader typed is dropped.
 */
export function pendingTags(typed: string): string[] {
  return normalizeTags(typed.split(","));
}

/** Number of characters as the counter shows them (code points, like the limit). */
export function charCount(text: string): number {
  return Array.from(text).length;
}
