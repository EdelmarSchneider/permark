// Where the review stands after a keep, a revert, or a refresh. Pure, so the store and the tests share it.

import type { ReviewChange } from "./types";

/**
 * The change to show after one was kept or reverted. The acted change is gone, so the same index is the next one;
 * acting on the last leaves the one before it.
 */
export function indexAfterAction(countBefore: number, acted: number): number {
  if (countBefore <= 1) return 0;
  if (acted >= countBefore - 1) return acted - 1;
  return acted;
}

/**
 * The change a refresh should land on: the one whose old range contains `oldStart`, or the nearest one after it.
 * An agent's write cannot move the approved text, so the place in that text still finds the change.
 */
export function recoverIndex(changes: readonly ReviewChange[], oldStart: number): number {
  if (changes.length === 0) return 0;
  let after = -1;
  for (let i = 0; i < changes.length; i++) {
    const range = changes[i].old;
    if (oldStart >= range.startByte && oldStart < range.endByte) return i;
    if (after < 0 && range.startByte >= oldStart) after = i;
  }
  return after >= 0 ? after : changes.length - 1;
}

/** The paths that fell out of an LRU of `limit` when `path` was visited again. */
export function touchLru(paths: readonly string[], path: string, limit: number): {
  kept: string[];
  dropped: string[];
} {
  const kept = [path, ...paths.filter((candidate) => candidate !== path)].slice(0, limit);
  const stayed = new Set(kept);
  return { kept, dropped: paths.filter((candidate) => !stayed.has(candidate)) };
}
