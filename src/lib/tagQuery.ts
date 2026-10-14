// `tag:spec` filter tokens in Search and Quick open. Finished tokens (followed by a space) become filter
// chips; the token being typed at the end of the query opens the tag suggestions.

import { normalizeTag } from "./tagText";

export type TagQuery = {
  /** Tags of the finished `tag:x` tokens: normalized, in order, without repeats. */
  tags: string[];
  /** The rest of the query: the words, with the tokens removed and spaces tidied. */
  text: string;
  /** The `tag:x` token still being typed at the end ("" right after `tag:`), or null. */
  partial: string | null;
};

const TOKEN = /^tag:(.*)$/i;

/** Splits a query into `tag:` filters and the words to search for. */
export function parseTagQuery(query: string): TagQuery {
  const tags: string[] = [];
  const words: string[] = [];
  const pieces = query.split(/\s+/).filter((piece) => piece !== "");
  const typing = !/\s$/.test(query);
  let partial: string | null = null;
  pieces.forEach((piece, i) => {
    const match = TOKEN.exec(piece);
    if (!match) {
      words.push(piece);
    } else if (typing && i === pieces.length - 1) {
      partial = match[1];
    } else {
      const tag = normalizeTag(match[1]);
      if (tag !== null && !tags.includes(tag)) tags.push(tag);
    }
  });
  return { tags, text: words.join(" "), partial };
}

/** The query without the token being typed (what is left once a suggestion became a chip). */
export function withoutPartial(query: string): string {
  return query.replace(/(^|\s)tag:\S*$/i, "$1");
}

/**
 * Moves the finished `tag:x` tokens of `query` into the chips. Returns the new chips and the text for
 * the input (which keeps the token being typed, so its suggestions stay open).
 */
export function absorbTokens(
  chips: readonly string[],
  query: string,
): { chips: string[]; text: string; changed: boolean } {
  const parsed = parseTagQuery(query);
  if (parsed.tags.length === 0) return { chips: [...chips], text: query, changed: false };
  const merged = [...chips];
  for (const tag of parsed.tags) if (!merged.includes(tag)) merged.push(tag);
  const typing = parsed.partial === null ? "" : `tag:${parsed.partial}`;
  const text = [parsed.text, typing].filter((part) => part !== "").join(" ");
  return { chips: merged, text, changed: true };
}

/** The tags of `tags` that no file has. */
export function unknownTags(tags: readonly string[], known: ReadonlySet<string>): string[] {
  return tags.filter((tag) => !known.has(tag));
}

/** Whether a file with these tags passes a filter that needs all of `wanted` (an empty filter passes everything). */
export function hasAllTags(
  have: readonly string[] | undefined,
  wanted: readonly string[],
): boolean {
  return wanted.every((tag) => have?.includes(tag) === true);
}
