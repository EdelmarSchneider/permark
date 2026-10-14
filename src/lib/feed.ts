// Pure model of the "Live changes" feed. Keyed by path (updated in place, newest first); a bulk batch
// (checkout, rebase, ...) collapses into one expandable entry instead of flooding the list.

import { folderLabel, isMarkdown, splitWorktree } from "./paths";
import type { FsBatch, FsChange } from "./types";

export type ChangeKind = FsChange["kind"];

export type FileEntry = {
  type: "file";
  id: string;
  path: string;
  kind: ChangeKind;
  /** Previous path of a renamed file. */
  from?: string;
  /** How many events this path produced while the project was open. */
  saves: number;
  atMs: number;
  isDoc: boolean;
  /** Worktree name for `@wt/<name>/...` paths. */
  worktree: string | null;
  /** Containing folder without the worktree prefix; "" is the project root. */
  folder: string;
};

export type BulkItem = { path: string; kind: ChangeKind };

export type BulkEntry = {
  type: "bulk";
  id: string;
  reason: "gitOperation" | "large" | "overflow";
  /** Total files affected, as reported by the backend (the item list is capped). */
  count: number;
  /** Markdown files among the listed items. */
  docCount: number;
  items: BulkItem[];
  atMs: number;
  /** The backend asked the UI to resync (events were lost). */
  rescan: boolean;
};

export type FeedEntry = FileEntry | BulkEntry;

export type FeedUpdate = {
  /** File entries touched by the batch (empty for a bulk batch). */
  files: FileEntry[];
  /** The collapsed entry for a bulk batch. */
  bulk: BulkEntry | null;
};

const MAX_ENTRIES = 500;

/** Combines the kind recorded for a path with a new event for the same path. */
export function mergeKind(prev: ChangeKind | undefined, next: ChangeKind): ChangeKind {
  if (prev === undefined) return next;
  switch (next) {
    case "removed":
    case "renamed":
      return next;
    case "created":
      // deleted and written again (an editor's "safe save") is a plain modification
      return prev === "removed" ? "modified" : prev;
    case "modified":
      return prev === "created" || prev === "renamed" ? prev : "modified";
  }
}

export class FeedStore {
  private list: FeedEntry[] = [];
  private seq = 0;

  /** Entries, newest first. A new array instance after every change. */
  get entries(): FeedEntry[] {
    return this.list;
  }

  /** Time of the newest event, or null when nothing happened yet. */
  get lastEventMs(): number | null {
    return this.list.length > 0 ? this.list[0].atMs : null;
  }

  clear(): void {
    this.list = [];
  }

  apply(batch: FsBatch, now: number): FeedUpdate {
    if (batch.bulk !== null) {
      const bulk = this.addBulk(batch, batch.bulk, now);
      return { files: [], bulk };
    }
    const files: FileEntry[] = [];
    const ordered = [...batch.changes].sort((a, b) => a.atMs - b.atMs);
    for (const change of ordered) {
      const entry = this.addChange(change);
      if (!files.includes(entry)) files.push(entry);
    }
    // an entry may have been replaced by a later event in the same batch
    return { files: files.filter((f) => this.list.includes(f)), bulk: null };
  }

  /** Entries to show; "docs only" hides non-Markdown files and reports how many were hidden. */
  view(docsOnly: boolean): { entries: FeedEntry[]; hidden: number } {
    if (!docsOnly) return { entries: this.list, hidden: 0 };
    const entries = this.list.filter((e) => e.type === "bulk" || e.isDoc);
    return { entries, hidden: this.list.length - entries.length };
  }

  private addChange(change: FsChange): FileEntry {
    const previous = this.list.find(
      (e): e is FileEntry => e.type === "file" && e.path === change.path,
    );
    if (change.kind === "renamed" && change.from !== undefined) {
      // the file now lives at `path`; the old path is no longer interesting
      this.list = this.list.filter((e) => !(e.type === "file" && e.path === change.from));
    }
    const { worktree } = splitWorktree(change.path);
    const entry: FileEntry = {
      type: "file",
      id: change.path,
      path: change.path,
      kind: mergeKind(previous?.kind, change.kind),
      from: change.kind === "renamed" ? change.from : previous?.from,
      saves: (previous?.saves ?? 0) + 1,
      atMs: change.atMs,
      isDoc: isMarkdown(change.path),
      worktree,
      folder: folderLabel(change.path),
    };
    const rest = this.list.filter((e) => e !== previous);
    this.list = [entry, ...rest].slice(0, MAX_ENTRIES);
    return entry;
  }

  private addBulk(batch: FsBatch, bulk: NonNullable<FsBatch["bulk"]>, now: number): BulkEntry {
    const items: BulkItem[] =
      batch.changes.length > 0
        ? batch.changes.map((c) => ({ path: c.path, kind: c.kind }))
        : bulk.paths.map((path) => ({ path, kind: "modified" as const }));
    const atMs = batch.changes.reduce((max, c) => Math.max(max, c.atMs), 0) || now;
    const entry: BulkEntry = {
      type: "bulk",
      id: `bulk-${++this.seq}`,
      reason: bulk.reason,
      count: bulk.count,
      docCount: items.filter((i) => isMarkdown(i.path)).length,
      items,
      atMs,
      rescan: batch.rescan,
    };
    this.list = [entry, ...this.list].slice(0, MAX_ENTRIES);
    return entry;
  }
}

/** Drops everything but the first `limit` bulk items unless `all` is set. */
export function visibleBulkItems(entry: BulkEntry, all: boolean, limit = 50): BulkItem[] {
  return all ? entry.items : entry.items.slice(0, limit);
}
