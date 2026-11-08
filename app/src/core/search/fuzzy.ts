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
const MARK = /^\p{M}$/u;

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

/** Below this many points a letter on average (runs and word starts), a subsequence is letters strewn over the
 *  text: such a match would list nearly everything for a short query, so it is no match. */
const SCATTERED_BELOW = 1;

/** Where the letters of `q` fall in `text`, in order: the leftmost each time, or (`starts`) the first one from there
 *  that starts a word or follows the letter before ("rt" in `rust-tauri`: the `t` of `tauri`). `null`: no match. */
function letters(text: readonly string[], q: readonly string[], starts: boolean): number[] | null {
  const positions: number[] = [];
  let from = 0;
  for (const ch of q) {
    const leftmost = text.indexOf(ch, from);
    if (leftmost < 0) return null;
    const after = positions[positions.length - 1];
    let at = leftmost;
    if (starts) {
      for (let i = leftmost; i < text.length; i++) {
        if (text[i] !== ch) continue;
        if (
          i === 0 ||
          BOUNDARY.test(text[i - 1] ?? "") ||
          (after !== undefined && i === after + 1)
        ) {
          at = i;
          break;
        }
      }
    }
    positions.push(at);
    from = at + 1;
  }
  return positions;
}

/** Points of matched letters: runs and word starts count, the gaps between them cost a little. */
function lettersScore(text: readonly string[], positions: readonly number[]): number {
  let score = 0;
  positions.forEach((pos, i) => {
    if (i > 0 && pos === (positions[i - 1] ?? -2) + 1) score += 3;
    if (pos === 0 || BOUNDARY.test(text[pos - 1] ?? "")) score += 2;
  });
  const first = positions[0] ?? 0;
  const last = positions[positions.length - 1] ?? 0;
  return score - (last - first - (positions.length - 1)) * 0.1;
}

/** The better of the leftmost letters and the word-start ones; letters strewn over the text are no match. */
function subsequence(text: readonly string[], q: readonly string[], offset: number): Match | null {
  const leftmost = letters(text, q, false);
  if (leftmost === null) return null;
  let best = { score: lettersScore(text, leftmost), positions: leftmost };
  const starts = letters(text, q, true);
  if (starts !== null) {
    const score = lettersScore(text, starts);
    if (score > best.score) best = { score, positions: starts };
  }
  if (best.score < q.length * SCATTERED_BELOW) return null;
  return { score: best.score, positions: best.positions.map((p) => p + offset) };
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
  // a mark written apart from its letter (decomposed text) goes, as it does in a letter written whole: the query has no
  // positions to keep aligned (same rule as the backend's `Needle`)
  const whole = foldAligned(query.trim()).filter((c) => !MARK.test(c));
  if (whole.join("").trim() === "") return null;
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
