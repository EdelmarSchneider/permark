// Fuzzy matching of short texts held in the window (command labels, the headings of the open document): the
// same tiers and scores as the backend's path match (`search/fuzzy.rs`). Positions are indices of characters
// (code points) of the original text.

export type Match = { score: number; positions: number[] };

/** One character folded to one character: lowercase, accent gone. */
function foldChar(c: string): string {
  if (c.charCodeAt(0) < 128) return c.toLowerCase();
  const base = c.normalize("NFKD")[0] ?? c;
  return base.toLowerCase()[0] ?? base;
}

function foldAligned(s: string): string[] {
  return Array.from(s, foldChar);
}

const BOUNDARY = /[\s/\-_.]/;

function range(start: number, length: number): number[] {
  return Array.from({ length }, (_, i) => start + i);
}

function find(hay: readonly string[], needle: readonly string[]): number {
  if (needle.length === 0 || needle.length > hay.length) return -1;
  for (let i = 0; i + needle.length <= hay.length; i++) {
    if (needle.every((c, j) => hay[i + j] === c)) return i;
  }
  return -1;
}

function subsequence(text: readonly string[], q: readonly string[], offset: number): Match | null {
  const positions: number[] = [];
  let from = 0;
  for (const ch of q) {
    const at = text.indexOf(ch, from);
    if (at < 0) return null;
    positions.push(at);
    from = at + 1;
  }
  let score = 0;
  positions.forEach((pos, i) => {
    if (i > 0 && pos === (positions[i - 1] ?? -2) + 1) score += 3;
    if (pos === 0 || BOUNDARY.test(text[pos - 1] ?? "")) score += 2;
  });
  const first = positions[0] ?? 0;
  const last = positions[positions.length - 1] ?? 0;
  score -= (last - first - (positions.length - 1)) * 0.1;
  return { score, positions: positions.map((p) => p + offset) };
}

function matchOne(q: readonly string[], p: readonly string[]): Match | null {
  const nameStart = p.lastIndexOf("/") + 1;
  const name = p.slice(nameStart);
  if (q.every((c, i) => name[i] === c)) {
    return { score: 1000 - name.length * 0.5, positions: range(nameStart, q.length) };
  }
  const inName = find(name, q);
  if (inName >= 0) {
    return {
      score: 800 - inName * 4 - name.length * 0.5,
      positions: range(nameStart + inName, q.length),
    };
  }
  const seqName = subsequence(name, q, nameStart);
  if (seqName) return { score: 500 + seqName.score, positions: seqName.positions };
  const inPath = find(p, q);
  if (inPath >= 0) return { score: 300 - inPath * 0.5, positions: range(inPath, q.length) };
  const seqPath = subsequence(p, q, 0);
  return seqPath ? { score: 100 + seqPath.score, positions: seqPath.positions } : null;
}

/** Scores `text` against `query`; `null` when the query does not match. */
export function fuzzyMatch(query: string, text: string): Match | null {
  const whole = foldAligned(query.trim());
  if (whole.length === 0) return null;
  const p = foldAligned(text);
  const found = matchOne(whole, p);
  if (found) return found;
  const words = whole
    .join("")
    .split(/\s+/)
    .filter((w) => w !== "");
  if (words.length < 2) return null;
  let score = 0;
  const positions = new Set<number>();
  for (const word of words) {
    const m = matchOne(Array.from(word), p);
    if (!m) return null;
    score += m.score;
    for (const at of m.positions) positions.add(at);
  }
  return { score: score / words.length, positions: [...positions].sort((a, b) => a - b) };
}

/** `text` cut into runs, the matched characters marked (code points; for highlighting). */
export function markRuns(
  text: string,
  positions: readonly number[],
): { text: string; mark: boolean }[] {
  const marked = new Set(positions);
  const runs: { text: string; mark: boolean }[] = [];
  Array.from(text).forEach((ch, i) => {
    const mark = marked.has(i);
    const last = runs[runs.length - 1];
    if (last && last.mark === mark) last.text += ch;
    else runs.push({ text: ch, mark });
  });
  return runs;
}
