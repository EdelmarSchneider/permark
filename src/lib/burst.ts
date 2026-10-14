// Coalesces a burst of changed documents into one counter, so the UI shows a single toast that counts up.

import type { ChangeKind } from "./feed";

export type BurstState = {
  /** Distinct documents changed in the current burst. */
  count: number;
  latest: { path: string; kind: ChangeKind };
};

export class BurstTracker {
  private paths = new Set<string>();
  private lastAt = Number.NEGATIVE_INFINITY;

  /** `windowMs`: a pause longer than this ends the burst. */
  constructor(private readonly windowMs = 10_000) {}

  note(path: string, kind: ChangeKind, now: number): BurstState {
    if (now - this.lastAt > this.windowMs) this.paths.clear();
    this.lastAt = now;
    this.paths.add(path);
    return { count: this.paths.size, latest: { path, kind } };
  }

  /** Called when the toast is dismissed: the next change starts a new count. */
  reset(): void {
    this.paths.clear();
    this.lastAt = Number.NEGATIVE_INFINITY;
  }
}
