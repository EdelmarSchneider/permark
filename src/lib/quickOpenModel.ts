// What Quick open lists for a typed query: files filtered by `tag:` chips, ranked by name, and (at the
// lowest rank) found through their description.

import { rankPaths } from "./fuzzy";
import { hasAllTags } from "./tagQuery";
import { foldChars } from "./text";

export type FoundFile = {
  path: string;
  /** Positions of the path characters that matched (empty for a description match or a tag-only query). */
  positions: number[];
  /** Positions of the description characters that matched, when the description is why the file is listed. */
  descriptionPositions: number[];
};

/**
 * Every word of `query` must appear in `description` (ignoring case and accents). Returns the
 * positions of the first occurrence of each word, or null when a word is missing.
 */
export function descriptionMatch(query: string, description: string): number[] | null {
  const words = foldChars(query.trim()).split(/\s+/).filter(Boolean);
  if (words.length === 0 || description === "") return null;
  const folded = foldChars(description);
  const positions = new Set<number>();
  for (const word of words) {
    const at = folded.indexOf(word);
    if (at < 0) return null;
    for (let i = 0; i < word.length; i++) positions.add(at + i);
  }
  return [...positions].sort((a, b) => a - b);
}

export type SearchFilesInput = {
  /** The words typed (without the `tag:` tokens). */
  text: string;
  /** Files must have all of these tags. */
  tags: readonly string[];
  files: readonly string[];
  tagsOf: (path: string) => readonly string[] | undefined;
  describe: (path: string) => string;
  /** Larger is more recent; breaks ties between equal matches and orders a tag-only query. */
  recency: ReadonlyMap<string, number>;
  limit: number;
};

/**
 * Files whose path matches the words, best first; then files whose description has all the words. With
 * only tags (no words) the files that have them, most recent first.
 */
export function searchFiles(input: SearchFilesInput): FoundFile[] {
  const { text, tags, files, tagsOf, describe, recency, limit } = input;
  const candidates =
    tags.length > 0 ? files.filter((f) => hasAllTags(tagsOf(f), tags)) : [...files];
  if (text.trim() === "") {
    return candidates
      .sort((a, b) => (recency.get(b) ?? 0) - (recency.get(a) ?? 0) || a.localeCompare(b))
      .slice(0, limit)
      .map((path) => ({ path, positions: [], descriptionPositions: [] }));
  }
  const byName: FoundFile[] = rankPaths(text, candidates, recency, limit).map((r) => ({
    path: r.path,
    positions: r.positions,
    descriptionPositions: [],
  }));
  if (byName.length >= limit) return byName;
  const named = new Set(byName.map((f) => f.path));
  const byDescription: FoundFile[] = [];
  for (const path of candidates) {
    if (named.has(path)) continue;
    const positions = descriptionMatch(text, describe(path));
    if (positions) byDescription.push({ path, positions: [], descriptionPositions: positions });
  }
  byDescription.sort(
    (a, b) =>
      (recency.get(b.path) ?? 0) - (recency.get(a.path) ?? 0) || a.path.localeCompare(b.path),
  );
  return [...byName, ...byDescription].slice(0, limit);
}
