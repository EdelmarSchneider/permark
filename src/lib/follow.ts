// Pure rules for the Follow toggle: which change is worth following and how often we may jump.

import { isMarkdown } from "./paths";
import type { FsBatch } from "./types";

/**
 * The Markdown document a batch says we should follow: the most recent created, modified or renamed
 * (to) document. Never for bulk batches (checkout, rebase, ...), never for removals.
 */
export function followCandidate(batch: FsBatch): string | null {
  if (batch.bulk !== null) return null;
  let best: { path: string; atMs: number } | null = null;
  for (const change of batch.changes) {
    if (change.kind === "removed" || !isMarkdown(change.path)) continue;
    if (best === null || change.atMs >= best.atMs) best = { path: change.path, atMs: change.atMs };
  }
  return best?.path ?? null;
}

/** At most one jump per `dwellMs`; a jump that arrives too early waits for the dwell to pass. */
export class FollowGate {
  private lastJumpAt = Number.NEGATIVE_INFINITY;
  private pending: string | null = null;

  constructor(private readonly dwellMs = 3000) {}

  /**
   * Offers the latest changed document. Returns the path to open right now, or null when the offer was
   * deferred (rate limit) or dropped (`blocked`: text is selected or an overlay is open).
   */
  offer(path: string, now: number, blocked: boolean): string | null {
    if (blocked) {
      this.pending = null;
      return null;
    }
    if (now - this.lastJumpAt >= this.dwellMs) {
      this.lastJumpAt = now;
      this.pending = null;
      return path;
    }
    this.pending = path;
    return null;
  }

  /** Milliseconds until a deferred offer may fire, or null when nothing is waiting. */
  msUntilDue(now: number): number | null {
    if (this.pending === null) return null;
    return Math.max(0, this.lastJumpAt + this.dwellMs - now);
  }

  /** Fires the deferred offer if its dwell has passed and nothing blocks it. */
  due(now: number, blocked: boolean): string | null {
    if (this.pending === null) return null;
    if (blocked) {
      this.pending = null;
      return null;
    }
    if (now - this.lastJumpAt < this.dwellMs) return null;
    const path = this.pending;
    this.pending = null;
    this.lastJumpAt = now;
    return path;
  }

  clear(): void {
    this.pending = null;
  }
}
