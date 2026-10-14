// Pure search-in-page logic. The DOM layer (domFind.ts) feeds it the text nodes of the document as
// segments and turns the returned matches into ranges. Matching is case and accent insensitive and can
// span text nodes (a phrase interrupted by <strong> or <code>) but never block boundaries.

export type FindSegment = {
  text: string;
  /** True when this segment starts a new block (paragraph, list item, table cell, ...). */
  blockStart: boolean;
};

export type FindMatch = { startSeg: number; startOff: number; endSeg: number; endOff: number };

/** "phrase": the whole query as typed. "terms": any of its words (used when opened from a search hit). */
export type FindMode = "phrase" | "terms";

const MAX_MATCHES = 5000;
const SENTINEL = "\n";

function foldPoint(ch: string): string {
  return ch.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/** Folds a query the same way the document text is folded (collapsed whitespace, no accents, lowercase). */
export function foldQuery(query: string): string {
  let out = "";
  for (const ch of query) {
    if (/\s/u.test(ch)) {
      if (!out.endsWith(" ")) out += " ";
    } else {
      out += foldPoint(ch);
    }
  }
  return out.trim();
}

type Folded = {
  text: string;
  segOf: number[];
  offOf: number[];
  lenOf: number[];
};

function foldSegments(segments: FindSegment[]): Folded {
  const folded: Folded = { text: "", segOf: [], offOf: [], lenOf: [] };
  // the arrays are indexed by UTF-16 unit of `folded.text`, so astral characters take two slots
  const push = (str: string, seg: number, off: number, len: number): void => {
    folded.text += str;
    for (let i = 0; i < str.length; i++) {
      folded.segOf.push(seg);
      folded.offOf.push(off);
      folded.lenOf.push(len);
    }
  };
  segments.forEach((segment, s) => {
    if (segment.blockStart && folded.text !== "") push(SENTINEL, -1, 0, 0);
    let off = 0;
    for (const ch of segment.text) {
      if (/\s/u.test(ch)) {
        if (!folded.text.endsWith(" ") && !folded.text.endsWith(SENTINEL) && folded.text !== "") {
          push(" ", s, off, ch.length);
        }
      } else {
        push(foldPoint(ch), s, off, ch.length);
      }
      off += ch.length;
    }
  });
  return folded;
}

function occurrences(haystack: string, needle: string, limit: number): [number, number][] {
  const out: [number, number][] = [];
  if (needle === "") return out;
  let from = 0;
  while (out.length < limit) {
    const at = haystack.indexOf(needle, from);
    if (at < 0) break;
    out.push([at, at + needle.length]);
    from = at + needle.length;
  }
  return out;
}

export function findMatches(
  segments: FindSegment[],
  query: string,
  mode: FindMode = "phrase",
  max = MAX_MATCHES,
): FindMatch[] {
  const q = foldQuery(query);
  if (q === "") return [];
  const folded = foldSegments(segments);

  let ranges: [number, number][];
  if (mode === "phrase") {
    ranges = occurrences(folded.text, q, max);
  } else {
    const terms = [...new Set(q.split(" ").filter(Boolean))];
    const all = terms.flatMap((term) => occurrences(folded.text, term, max));
    all.sort((a, b) => a[0] - b[0] || b[1] - a[1]);
    ranges = [];
    for (const range of all) {
      const last = ranges[ranges.length - 1];
      if (last && range[0] < last[1]) last[1] = Math.max(last[1], range[1]);
      else ranges.push([range[0], range[1]]);
    }
    ranges = ranges.slice(0, max);
  }

  const matches: FindMatch[] = [];
  for (const [from, to] of ranges) {
    const last = to - 1;
    if (folded.segOf[from] < 0 || folded.segOf[last] < 0) continue;
    matches.push({
      startSeg: folded.segOf[from],
      startOff: folded.offOf[from],
      endSeg: folded.segOf[last],
      endOff: folded.offOf[last] + folded.lenOf[last],
    });
  }
  return matches;
}

/** Next/previous match index with wrap-around; -1 when there are no matches. */
export function stepIndex(current: number, total: number, direction: 1 | -1): number {
  if (total <= 0) return -1;
  if (current < 0) return direction === 1 ? 0 : total - 1;
  return (current + direction + total) % total;
}

/** The index to keep selected after the document changed under an active search. */
export function clampIndex(current: number, total: number): number {
  if (total <= 0) return -1;
  return Math.min(Math.max(current, 0), total - 1);
}
