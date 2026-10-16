// LCS diff over the per-block hashes of a document (`DocumentView.blocks`).
// Used on live reload: new/changed blocks glow, removed runs leave a marker, and the scroll position is
// anchored to a block that exists in both versions.
//
// The LCS only knows blocks that are exactly equal, so an edited block looks like one block removed and one
// added. Given the text of the blocks, `diffBlocks` pairs such a block with its old version (and a block that
// moved with its old place), so that it glows and nothing is reported as removed for it.

export type RemovedRun = {
  /**
   * Index (in the new document) of the block the marker goes in front of: the next block that survived the
   * update or was edited in place, after any added blocks before it. Equals the new length for a run at the end.
   */
  beforeNew: number;
  /** Indices (in the old document) of the removed blocks, in order. */
  oldIndices: number[];
};

export type BlockDiff = {
  /** Blocks present in both versions, as [oldIndex, newIndex] pairs in document order. */
  matches: [number, number][];
  /** New indices of blocks that were added or changed. */
  added: number[];
  /**
   * Old blocks that live on as a changed or moved block, as [oldIndex, newIndex] pairs by new index. The new
   * index is also in `added` (it glows) and the old one is in no removed run. Empty unless `diffBlocks` got
   * the text of the blocks.
   */
  paired: [number, number][];
  /** Runs of old blocks that no longer exist. */
  removed: RemovedRun[];
};

/** The plain text of the blocks of both versions, read only for the blocks a pairing needs. */
export type BlockText = {
  old: (index: number) => string;
  new: (index: number) => string;
};

/** Above this many table cells the middle part is treated as fully replaced instead of diffed. */
const MAX_CELLS = 4_000_000;

/** Two blocks are one block edited when at least this share of their words is the same (1 is identical). */
const SAME_BLOCK = 0.4;

/** A run of changed blocks bigger than this many old x new blocks is not paired: it is a rewrite, not an edit. */
const MAX_COMPARISONS = 40_000;

/** Longest common subsequence of two key lists, as [oldIndex, newIndex] pairs. */
function lcsPairs(a: string[], b: string[]): [number, number][] {
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }

  const pairs: [number, number][] = [];
  for (let i = 0; i < start; i++) pairs.push([i, i]);

  const n = endA - start;
  const m = endB - start;
  if (n > 0 && m > 0 && n * m <= MAX_CELLS) {
    const width = m + 1;
    const table = new Uint16Array((n + 1) * width);
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        table[i * width + j] =
          a[start + i] === b[start + j]
            ? table[(i + 1) * width + j + 1] + 1
            : Math.max(table[(i + 1) * width + j], table[i * width + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < n && j < m) {
      if (a[start + i] === b[start + j]) {
        pairs.push([start + i, start + j]);
        i++;
        j++;
      } else if (table[(i + 1) * width + j] >= table[i * width + j + 1]) {
        i++;
      } else {
        j++;
      }
    }
  }

  const tailA = endA;
  const tailB = endB;
  for (let k = 0; k < a.length - tailA; k++) pairs.push([tailA + k, tailB + k]);
  return pairs;
}

// ---- recognising edited and moved blocks ----------------------------------------------------------------------------

/** The plain text of a block's HTML: tags become spaces (so neighbouring cells stay apart) and entities are decoded. */
export function htmlText(html: string): string {
  const entities: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" };
  return html
    .replace(/<[^>]*>/g, " ")
    .replace(/&(amp|lt|gt|quot|#39);/g, (_, name: string) => entities[name] ?? "");
}

type Words = { counts: Map<string, number>; total: number };

function wordsOf(text: string): Words {
  const counts = new Map<string, number>();
  let total = 0;
  for (const [word] of text.toLowerCase().matchAll(/[\p{L}\p{N}]+/gu)) {
    counts.set(word, (counts.get(word) ?? 0) + 1);
    total++;
  }
  return { counts, total };
}

/** The share of words two blocks have in common (repeats counted), from 0 to 1. Blocks without words share none. */
function similarity(a: Words, b: Words): number {
  if (a.total === 0 || b.total === 0) return 0;
  const [small, large] = a.counts.size <= b.counts.size ? [a, b] : [b, a];
  let common = 0;
  for (const [word, count] of small.counts) common += Math.min(count, large.counts.get(word) ?? 0);
  return (2 * common) / (a.total + b.total);
}

/**
 * Pairs the old and new blocks of one gap between two kept blocks, in order, so that the total similarity of the
 * pairs is the highest and no pair is less similar than `SAME_BLOCK`. Returns [oldIndex, newIndex] pairs.
 */
function alignGap(oldIdx: number[], newIdx: number[], text: BlockText): [number, number][] {
  const rows = oldIdx.length;
  const cols = newIdx.length;
  if (rows === 0 || cols === 0 || rows * cols > MAX_COMPARISONS) return [];

  const oldWords = oldIdx.map((o) => wordsOf(text.old(o)));
  const newWords = newIdx.map((n) => wordsOf(text.new(n)));
  const width = cols + 1;
  const best = new Float64Array((rows + 1) * width); // best[i * width + j]: the best total for blocks i.. and j..
  const same = new Float64Array(rows * cols);
  for (let i = rows - 1; i >= 0; i--) {
    for (let j = cols - 1; j >= 0; j--) {
      const s = similarity(oldWords[i], newWords[j]);
      same[i * cols + j] = s;
      let value = Math.max(best[(i + 1) * width + j], best[i * width + j + 1]);
      if (s >= SAME_BLOCK) value = Math.max(value, best[(i + 1) * width + j + 1] + s);
      best[i * width + j] = value;
    }
  }

  const pairs: [number, number][] = [];
  let i = 0;
  let j = 0;
  while (i < rows && j < cols) {
    const s = same[i * cols + j];
    if (s >= SAME_BLOCK && best[i * width + j] === best[(i + 1) * width + j + 1] + s) {
      pairs.push([oldIdx[i], newIdx[j]]);
      i++;
      j++;
    } else if (best[(i + 1) * width + j] >= best[i * width + j + 1]) {
      i++;
    } else {
      j++;
    }
  }
  return pairs;
}

/**
 * Finds, among the blocks the LCS did not keep, the old blocks that live on: a block with the same key somewhere
 * else (`moved`), and a block edited in place, recognised by its text inside the gap between two kept blocks
 * (`edited`).
 */
function pairBlocks(
  oldKeys: string[],
  newKeys: string[],
  matches: [number, number][],
  text: BlockText,
): { moved: [number, number][]; edited: [number, number][] } {
  const keptOld = new Set(matches.map(([o]) => o));
  const keptNew = new Set(matches.map(([, n]) => n));

  // Equal blocks that are not in the LCS were moved: the first unkept old one is the first unkept new one.
  const waiting = new Map<string, number[]>();
  for (let o = oldKeys.length - 1; o >= 0; o--) {
    if (keptOld.has(o)) continue;
    const queue = waiting.get(oldKeys[o]);
    if (queue) queue.push(o);
    else waiting.set(oldKeys[o], [o]);
  }
  const moved: [number, number][] = [];
  const movedOld = new Set<number>();
  const movedNew = new Set<number>();
  for (let n = 0; n < newKeys.length; n++) {
    if (keptNew.has(n)) continue;
    const o = waiting.get(newKeys[n])?.pop();
    if (o === undefined) continue;
    moved.push([o, n]);
    movedOld.add(o);
    movedNew.add(n);
  }

  const edited: [number, number][] = [];
  let firstOld = 0;
  let firstNew = 0;
  const pairGap = (untilOld: number, untilNew: number): void => {
    const olds: number[] = [];
    for (let o = firstOld; o < untilOld; o++) if (!movedOld.has(o)) olds.push(o);
    const news: number[] = [];
    for (let n = firstNew; n < untilNew; n++) if (!movedNew.has(n)) news.push(n);
    for (const pair of alignGap(olds, news, text)) edited.push(pair);
  };
  for (const [o, n] of matches) {
    pairGap(o, n);
    firstOld = o + 1;
    firstNew = n + 1;
  }
  pairGap(oldKeys.length, newKeys.length);
  return { moved, edited };
}

/**
 * Diffs the blocks of two versions of a document by their keys. Given the text of the blocks, an edited block is
 * paired with its old version and a moved block with its old place (`paired`): it still glows (`added`), but its
 * old version is not reported as removed. Without the text, every block that is not exactly equal is a removal
 * plus an addition.
 */
export function diffBlocks(oldKeys: string[], newKeys: string[], text?: BlockText): BlockDiff {
  const matches = lcsPairs(oldKeys, newKeys);
  const { moved, edited } = text
    ? pairBlocks(oldKeys, newKeys, matches, text)
    : { moved: [], edited: [] };

  const keptNew = new Set(matches.map(([, n]) => n));
  const added: number[] = [];
  for (let n = 0; n < newKeys.length; n++) if (!keptNew.has(n)) added.push(n);

  // Where each old block that stays in order went. A marker for removed blocks goes in front of the next of these.
  const inPlace = new Map<number, number>([...matches, ...edited]);
  const movedOld = new Set(moved.map(([o]) => o));
  const nextNew = new Array<number>(oldKeys.length + 1);
  nextNew[oldKeys.length] = newKeys.length;
  for (let o = oldKeys.length - 1; o >= 0; o--) nextNew[o] = inPlace.get(o) ?? nextNew[o + 1];

  const removed: RemovedRun[] = [];
  for (let o = 0; o < oldKeys.length; o++) {
    if (inPlace.has(o) || movedOld.has(o)) continue;
    const beforeNew = nextNew[o + 1];
    const last = removed[removed.length - 1];
    if (last && last.beforeNew === beforeNew) last.oldIndices.push(o);
    else removed.push({ beforeNew, oldIndices: [o] });
  }

  const paired = [...moved, ...edited].sort((x, y) => x[1] - y[1]);
  return { matches, added, paired, removed };
}

/** Maps old block indices to new ones for the blocks that survived the update. */
export function oldToNew(diff: BlockDiff): Map<number, number> {
  return new Map(diff.matches);
}

/**
 * Chooses the block to anchor the scroll position on: the first visible old block that still exists.
 * Returns the old and new index, or null when none of the visible blocks survived.
 */
export function pickAnchor(
  visibleOld: number[],
  diff: BlockDiff,
): { oldIndex: number; newIndex: number } | null {
  const map = oldToNew(diff);
  for (const oldIndex of visibleOld) {
    const newIndex = map.get(oldIndex);
    if (newIndex !== undefined) return { oldIndex, newIndex };
  }
  return null;
}
