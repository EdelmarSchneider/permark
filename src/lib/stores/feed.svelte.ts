// The "Live changes" feed (pure model in ../feed.ts) as reactive state, plus the Follow toggle's gate.

import { SvelteSet } from "svelte/reactivity";
import { FeedStore, type FeedEntry, type FeedUpdate } from "../feed";
import { FollowGate } from "../follow";
import type { FsBatch } from "../types";

class FeedState {
  private model = new FeedStore();
  entries = $state.raw<FeedEntry[]>([]);
  docsOnly = $state(true);
  lastEventMs = $state<number | null>(null);
  /** Bulk entries the user expanded. */
  expanded = new SvelteSet<string>();
  /** Bulk entries showing every listed file instead of the first 50. */
  showAll = new SvelteSet<string>();
  readonly gate = new FollowGate();

  view = $derived.by(() => {
    void this.entries;
    return this.model.view(this.docsOnly);
  });

  apply(batch: FsBatch, now: number): FeedUpdate {
    const update = this.model.apply(batch, now);
    this.entries = [...this.model.entries];
    this.lastEventMs = this.model.lastEventMs;
    return update;
  }

  toggleDocsOnly(): void {
    this.docsOnly = !this.docsOnly;
  }

  toggleBulk(id: string): void {
    if (this.expanded.has(id)) this.expanded.delete(id);
    else this.expanded.add(id);
  }

  clear(): void {
    this.model.clear();
    this.entries = [];
    this.lastEventMs = null;
    this.expanded.clear();
    this.showAll.clear();
    this.gate.clear();
  }
}

export const feed = new FeedState();
