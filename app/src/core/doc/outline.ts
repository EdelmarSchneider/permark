// Outline helpers (pure): indentation from heading levels, the heading the reader is at.

import type { OutlineItem } from "../../api";

/** Indent depth per item: a level deeper than the one above is one step in, however many levels it skips. */
export function depths(items: readonly Pick<OutlineItem, "level">[]): number[] {
  const stack: number[] = [];
  return items.map((item) => {
    while (stack.length > 0 && (stack.at(-1) ?? 0) >= item.level) stack.pop();
    stack.push(item.level);
    return stack.length - 1;
  });
}

/**
 * The heading the reader is at: the last one whose top is at or above the line `limit` (tops in document order).
 * `-1` before the first heading.
 */
export function currentIndex(tops: readonly number[], limit: number): number {
  let low = 0;
  let high = tops.length - 1;
  let found = -1;
  // tops are in document order, so they only grow: the last one at or above the limit
  while (low <= high) {
    const middle = (low + high) >> 1;
    if ((tops[middle] ?? Infinity) <= limit) {
      found = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found;
}
