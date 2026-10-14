// Frontmatter values come as plain strings. Tag-like values become chips in the card.

const TAG_KEYS = new Set(["tags", "tag", "keywords", "categories", "labels", "aliases"]);

export type FrontmatterValue = { chips: string[] } | { text: string };

/** "[a, b]" or (for tag-like keys) "a, b" become chips; anything else stays text. */
export function frontmatterValue(key: string, value: string): FrontmatterValue {
  const trimmed = value.trim();
  const listed = /^\[(.*)\]$/s.exec(trimmed);
  const source = listed ? listed[1] : TAG_KEYS.has(key.toLowerCase()) ? trimmed : null;
  if (source === null) return { text: trimmed };
  const chips = source
    .split(",")
    .map((part) => part.trim().replace(/^["']|["']$/g, ""))
    .filter((part) => part !== "");
  return chips.length > 0 ? { chips } : { text: trimmed };
}
