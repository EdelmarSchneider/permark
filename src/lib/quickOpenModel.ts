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

export type CountMatchesInput = Omit<SearchFilesInput, "limit">;

/**
 * How many of `files` the query finds: the "+N hidden files match" row of Quick open counts the files the
 * Explorer filter keeps out. Nothing typed (no words and no tags) matches nothing.
 */
export function countMatches(input: CountMatchesInput): number {
  if (input.text.trim() === "" && input.tags.length === 0) return 0;
  return searchFiles({ ...input, limit: input.files.length }).length;
}

export type QuickMatchesInput = SearchFilesInput & {
  /** The files the Explorer filter keeps out. `files` is the rest. */
  hiddenFiles: readonly string[];
  /** Every file, hidden or not: what is searched once the hidden matches are listed. */
  allFiles: readonly string[];
  /** The "+N hidden files match" row was used. */
  reveal: boolean;
};

export type QuickMatches = {
  found: (FoundFile & { hidden: boolean })[];
  /** Hidden files that match and are not listed: the number on the "+N hidden files match" row. */
  moreHidden: number;
};

/**
 * What Quick open lists for a typed query. The files the Explorer filter hides stay out of the list and are
 * counted on a row of their own, unless they were revealed or nothing else matches: then they are listed (marked
 * hidden) right away, so the first row is the file and a single Enter opens it instead of only revealing it.
 */
export function quickMatches(input: QuickMatchesInput): QuickMatches {
  const { hiddenFiles, allFiles, reveal, ...search } = input;
  const visible = searchFiles(search);
  const moreHidden = reveal ? 0 : countMatches({ ...search, files: hiddenFiles });
  if (!reveal && (visible.length > 0 || moreHidden === 0)) {
    return { found: visible.map((f) => ({ ...f, hidden: false })), moreHidden };
  }
  const hidden = new Set(hiddenFiles);
  const found = searchFiles({ ...search, files: allFiles }).map((f) => ({
    ...f,
    hidden: hidden.has(f.path),
  }));
  return { found, moreHidden: 0 };
}

/**
 * Whether the selected row is the "+N hidden files match" row, which comes after the `listed` file rows. Enter on
 * it reveals the hidden matches; on any other row it opens the file.
 */
export function onMoreRow(active: number, listed: number, moreHidden: number): boolean {
  return moreHidden > 0 && active === listed;
}
