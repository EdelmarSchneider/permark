// Which lines of a text changed between two versions of it (the glow on a reload of code or plain text). Pure; the
// line diff is the editor's.

import { diff, splitLines } from "../editing/lines";

/** Lines `first ..= last`, 1-based. */
export type LineRange = { first: number; last: number };

export type TextChange = {
  /** Lines of the new text that are new or different. */
  added: readonly LineRange[];
  /** Places where lines went: after this line of the new text (0: before the first). */
  removedAfter: readonly number[];
};

/** Ranges kept at most (a rewrite of the whole file is one range anyway). */
const MAX_RANGES = 200;

/** What changed from `before` to `after`. */
export function changedLines(before: string, after: string): TextChange {
  if (before === after) return { added: [], removedAfter: [] };
  const added: LineRange[] = [];
  const removedAfter: number[] = [];
  const lines = splitLines(after);
  // the empty piece after a final line break is no line
  const real = after.endsWith("\n") ? lines.length - 1 : lines.length;
  for (const op of diff(splitLines(before), lines)) {
    if (op.kind !== "change") continue;
    if (op.add > 0) {
      const last = Math.min(op.b + op.add, real);
      if (op.b + 1 <= last) added.push({ first: op.b + 1, last });
    } else if (op.remove > 0 && op.b <= real) {
      removedAfter.push(op.b);
    }
  }
  return { added: added.slice(0, MAX_RANGES), removedAfter: removedAfter.slice(0, MAX_RANGES) };
}
