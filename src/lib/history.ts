// Back/forward history of opened documents, remembering the scroll position of each entry.

import { mapPath } from "./fileOps";
import type { Move } from "./types";

export type HistoryEntry = { path: string; scrollTop: number };

/** A document of the trail: `offset` steps from the open one (negative: back), the number to give [`NavHistory.go`]. */
export type TrailStop = { offset: number; path: string; current: boolean };

const MAX_ENTRIES = 100;

/** How many documents the menu of the Back or Forward button lists. */
export const TRAIL_MENU_LIMIT = 15;

export class NavHistory {
  private entries: HistoryEntry[] = [];
  private index = -1;

  get current(): HistoryEntry | null {
    return this.entries[this.index] ?? null;
  }

  get canBack(): boolean {
    return this.index > 0;
  }

  get canForward(): boolean {
    return this.index >= 0 && this.index < this.entries.length - 1;
  }

  /** Records where the reader is in the current entry, so Back can restore it. */
  saveScroll(scrollTop: number): void {
    const entry = this.current;
    if (entry) entry.scrollTop = scrollTop;
  }

  /** Visits `path`. Forward entries are dropped; revisiting the current path adds nothing. */
  push(path: string): void {
    if (this.current?.path === path) return;
    this.entries = this.entries.slice(0, this.index + 1);
    this.entries.push({ path, scrollTop: 0 });
    if (this.entries.length > MAX_ENTRIES) this.entries.shift();
    this.index = this.entries.length - 1;
  }

  /** Points the current entry at another path (a renamed file, a tombstone that was closed). */
  replaceCurrent(path: string): void {
    const entry = this.current;
    if (entry) {
      entry.path = path;
      entry.scrollTop = 0;
    } else {
      this.push(path);
    }
  }

  /** Follows moved or renamed files: every entry at or inside a moved path points at its new place. */
  rename(moves: readonly Move[]): void {
    for (const entry of this.entries) entry.path = mapPath(entry.path, moves);
  }

  /**
   * Moves `offset` steps along the history (negative: back), like a browser's `go(n)`. Every entry stays, so Back and
   * Forward walk the same trail afterwards. Null, with nothing moved, for no step or a step past either end.
   */
  go(offset: number): HistoryEntry | null {
    const target = this.index + offset;
    if (!Number.isInteger(offset) || offset === 0) return null;
    if (target < 0 || target >= this.entries.length) return null;
    this.index = target;
    return this.current;
  }

  back(): HistoryEntry | null {
    return this.go(-1);
  }

  forward(): HistoryEntry | null {
    return this.go(1);
  }

  /** The documents before (`back`) or after (`forward`) the open one, nearest first, at most `limit`. */
  stops(direction: "back" | "forward", limit = TRAIL_MENU_LIMIT): TrailStop[] {
    const step = direction === "back" ? -1 : 1;
    const out: TrailStop[] = [];
    for (let at = this.index + step; at >= 0 && at < this.entries.length; at += step) {
      if (out.length >= limit) break;
      out.push({ offset: at - this.index, path: this.entries[at].path, current: false });
    }
    return out;
  }

  /** The whole trail, newest first: what is ahead, then the open document (marked), then the way back. */
  trail(): TrailStop[] {
    return this.entries
      .map((entry, at) => ({
        offset: at - this.index,
        path: entry.path,
        current: at === this.index,
      }))
      .reverse();
  }

  /** Paths visited, most recent first, without duplicates. */
  recentPaths(): string[] {
    const seen = new Set<string>();
    const out: string[] = [];
    for (let i = this.entries.length - 1; i >= 0; i--) {
      const { path } = this.entries[i];
      if (!seen.has(path)) {
        seen.add(path);
        out.push(path);
      }
    }
    return out;
  }

  clear(): void {
    this.entries = [];
    this.index = -1;
  }
}
