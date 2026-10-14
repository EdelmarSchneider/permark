// Back/forward history of opened documents, remembering the scroll position of each entry.

import { mapPath } from "./fileOps";
import type { Move } from "./types";

export type HistoryEntry = { path: string; scrollTop: number };

const MAX_ENTRIES = 100;

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

  back(): HistoryEntry | null {
    if (!this.canBack) return null;
    this.index -= 1;
    return this.current;
  }

  forward(): HistoryEntry | null {
    if (!this.canForward) return null;
    this.index += 1;
    return this.current;
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
