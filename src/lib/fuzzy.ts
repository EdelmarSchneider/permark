// Fuzzy file matching for quick open (Ctrl+P). Ranking: file-name prefix > file-name substring >
// file-name subsequence > path substring > path subsequence; ties go to the most recently used.
// A query of several words ("auth token") that does not match as typed matches when each word matches the path,
// in any order.

import { foldChars } from "./text";

export type FuzzyMatch = {
  score: number;
  /** Indices into the path of the characters that matched (for bold rendering). */
  positions: number[];
};

const BOUNDARY = /[\s/\-_.]/;

function range(start: number, length: number): number[] {
  return Array.from({ length }, (_, i) => start + i);
}

/** Leftmost subsequence match of `query` in `text`, preferring runs and word starts. */
function subsequence(text: string, query: string, offset: number): FuzzyMatch | null {
  const positions: number[] = [];
  let from = 0;
  for (const ch of query) {
    const at = text.indexOf(ch, from);
    if (at < 0) return null;
    positions.push(offset + at);
    from = at + 1;
  }
  let score = 0;
  positions.forEach((pos, i) => {
    const local = pos - offset;
    if (i > 0 && pos === positions[i - 1] + 1) score += 3;
    if (local === 0 || BOUNDARY.test(text[local - 1])) score += 2;
  });
  score -= (positions[positions.length - 1] - positions[0] - (positions.length - 1)) * 0.1;
  return { score, positions };
}

/** Scores `path` against `query`; null when the query is not a subsequence of the path. */
export function fuzzyMatch(query: string, path: string): FuzzyMatch | null {
  const q = foldChars(query.trim());
  if (q === "") return null;
  const p = foldChars(path);
  const whole = matchOne(q, p);
  if (whole) return whole;
  const words = q.split(/\s+/);
  if (words.length < 2) return null;
  let score = 0;
  const positions = new Set<number>();
  for (const word of words) {
    const found = matchOne(word, p);
    if (!found) return null;
    score += found.score;
    found.positions.forEach((at) => positions.add(at));
  }
  // the mean keeps the scale of a single word, so a path that holds the words ranks with the paths that hold one
  return { score: score / words.length, positions: [...positions].sort((a, b) => a - b) };
}

/** `q` (folded, one piece) against `p` (folded). */
function matchOne(q: string, p: string): FuzzyMatch | null {
  const nameStart = p.lastIndexOf("/") + 1;
  const name = p.slice(nameStart);

  if (name.startsWith(q)) {
    return { score: 1000 - name.length * 0.5, positions: range(nameStart, q.length) };
  }
  const inName = name.indexOf(q);
  if (inName >= 0) {
    return {
      score: 800 - inName * 4 - name.length * 0.5,
      positions: range(nameStart + inName, q.length),
    };
  }
  const seqName = subsequence(name, q, nameStart);
  if (seqName) return { score: 500 + seqName.score, positions: seqName.positions };

  const inPath = p.indexOf(q);
  if (inPath >= 0) return { score: 300 - inPath * 0.5, positions: range(inPath, q.length) };

  const seqPath = subsequence(p, q, 0);
  if (seqPath) return { score: 100 + seqPath.score, positions: seqPath.positions };
  return null;
}

export type RankedPath = { path: string; score: number; positions: number[] };

/** Ranks `paths` for a query. `recency` maps a path to the last time it was used or changed. */
export function rankPaths(
  query: string,
  paths: string[],
  recency: ReadonlyMap<string, number> = new Map(),
  limit = 50,
): RankedPath[] {
  const ranked: RankedPath[] = [];
  for (const path of paths) {
    const match = fuzzyMatch(query, path);
    if (match) ranked.push({ path, ...match });
  }
  ranked.sort(
    (a, b) =>
      b.score - a.score ||
      (recency.get(b.path) ?? 0) - (recency.get(a.path) ?? 0) ||
      a.path.length - b.path.length ||
      a.path.localeCompare(b.path),
  );
  return ranked.slice(0, limit);
}
