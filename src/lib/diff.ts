// LCS diff over the per-block hashes of a document (`DocumentView.blocks`).
// Used on live reload: new/changed blocks glow, removed runs leave a marker, and the scroll position is
// anchored to a block that exists in both versions.

export type RemovedRun = {
  /**
   * Index (in the new document) of the next block that survived the update; the marker goes in front of
   * it, after any added blocks in the same gap. Equals the new length for a run at the end.
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
  /** Runs of old blocks that no longer exist. */
  removed: RemovedRun[];
};

/** Above this many table cells the middle part is treated as fully replaced instead of diffed. */
const MAX_CELLS = 4_000_000;

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

export function diffBlocks(oldKeys: string[], newKeys: string[]): BlockDiff {
  const matches = lcsPairs(oldKeys, newKeys);

  const added: number[] = [];
  const removed: RemovedRun[] = [];
  let oi = 0;
  let ni = 0;
  const flush = (untilOld: number, untilNew: number): void => {
    if (oi < untilOld) {
      const oldIndices: number[] = [];
      for (let k = oi; k < untilOld; k++) oldIndices.push(k);
      removed.push({ beforeNew: untilNew, oldIndices });
    }
    for (let k = ni; k < untilNew; k++) added.push(k);
  };
  for (const [o, n] of matches) {
    flush(o, n);
    oi = o + 1;
    ni = n + 1;
  }
  flush(oldKeys.length, newKeys.length);
  return { matches, added, removed };
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
