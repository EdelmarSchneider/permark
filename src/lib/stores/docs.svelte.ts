// The open document: loading, navigation history, live updates and the deleted-while-open state.
// DocView renders `request`; this store decides when to read, what to remember and when a file is gone.

import { api } from "../api";
import type { FindMode } from "../find";
import { errorText } from "../errors";
import { t } from "../i18n";
import { NavHistory } from "../history";
import type { DocumentView, FsBatch, FsChange } from "../types";
import { session } from "./session.svelte";
import { tags } from "./tags.svelte";
import { toast } from "./toast.svelte";

/** How long a removed file may stay missing before the open document is treated as deleted. */
export const DELETE_GRACE_MS = 1000;
/** The loading bar appears only when a read takes longer than this. */
const LOADING_DELAY_MS = 150;

export type LocateRequest = { line: number; find: { query: string; mode: FindMode } | null };

export type RenderRequest = {
  rev: number;
  view: DocumentView;
  /** "fresh": a different document or a first render. "update": the same document changed on disk. */
  mode: "fresh" | "update";
  anchor: string | null;
  locate: LocateRequest | null;
  /** Scroll position to restore (session or history). */
  scrollTop: number | null;
  /** Scroll to the first changed block after an update (Follow is on) or after opening a changed doc. */
  followScroll: boolean;
  /** Mark what changed since this document was last rendered, even though it is a fresh open. */
  diffAgainstLast: boolean;
};

export type OpenOptions = {
  anchor?: string | null;
  locate?: LocateRequest | null;
  scrollTop?: number | null;
  /** "push" adds a history entry (default), "none" is for back/forward, "replace" swaps the current one. */
  history?: "push" | "none" | "replace";
  /** Quietly ignore a failure (restoring the session's last document). */
  silent?: boolean;
  /** Opened because it changed (Follow, toast, feed): diff against the version seen last. */
  fromChange?: boolean;
};

class DocStore {
  current = $state.raw<DocumentView | null>(null);
  request = $state.raw<RenderRequest | null>(null);
  /** Scroll-only navigation within the current render: an anchor click or a search hit in the same doc. */
  jump = $state.raw<{ n: number; anchor: string | null; locate: LocateRequest | null } | null>(
    null,
  );
  loading = $state(false);
  loadingVisible = $state(false);
  error = $state.raw<{ path: string; message: string } | null>(null);
  deleted = $state.raw<{ atMs: number; movedTo: string | null } | null>(null);
  canBack = $state(false);
  canForward = $state(false);
  /** Changes marked by the last live update (reported by DocView after rendering). */
  marks = $state({ changed: 0, removed: 0 });
  scrolled = $state(false);

  private history = new NavHistory();
  private token = 0;
  private rev = 0;
  private jumpCount = 0;
  private loadingTimer: ReturnType<typeof setTimeout> | null = null;
  private refreshing = false;
  private refreshQueued = false;
  private queuedFollowScroll = false;
  private deleteTimer: ReturnType<typeof setTimeout> | null = null;
  private scrollTop = 0;

  reset(): void {
    this.token++;
    this.history.clear();
    this.current = null;
    this.request = null;
    this.error = null;
    this.deleted = null;
    this.marks = { changed: 0, removed: 0 };
    this.cancelDeleteCheck();
    this.endLoading();
    this.syncNav();
  }

  get path(): string | null {
    return this.current?.path ?? null;
  }

  /** Paths visited in this session, most recent first (quick open, "recently opened"). */
  recentPaths(): string[] {
    return this.history.recentPaths();
  }

  // ---- opening --------------------------------------------------------------------------------

  /** Opens a document. Resolves to false when it could not be read (a toast says why). */
  async open(path: string, opts: OpenOptions = {}): Promise<boolean> {
    if (this.current?.path === path && !this.deleted && opts.history !== "none") {
      // the same document: just move within it
      if (opts.anchor || opts.locate) {
        this.jump = {
          n: ++this.jumpCount,
          anchor: opts.anchor ?? null,
          locate: opts.locate ?? null,
        };
      }
      return true;
    }
    const token = ++this.token;
    this.beginLoading();
    let view: DocumentView;
    try {
      // The tags load along with the document, so the strip above it is there from the first paint.
      [view] = await Promise.all([api.readDocument(path), tags.preload(path)]);
    } catch (error) {
      if (token !== this.token) return false;
      this.endLoading();
      if (!opts.silent) {
        const message = errorText(error);
        if (!this.current) this.error = { path, message };
        toast.message(t("doc.openFailed", { path, message }), "warn");
      }
      return false;
    }
    if (token !== this.token) return false;
    this.endLoading();

    const mode = opts.history ?? "push";
    // back/forward saved the scroll position before moving through the history
    if (this.current && mode !== "none") this.history.saveScroll(this.scrollTop);
    if (mode === "push") this.history.push(path);
    else if (mode === "replace") this.history.replaceCurrent(path);
    this.cancelDeleteCheck();
    this.current = view;
    this.error = null;
    this.deleted = null;
    this.marks = { changed: 0, removed: 0 };
    this.scrolled = false;
    this.scrollTop = opts.scrollTop ?? 0;
    this.request = {
      rev: ++this.rev,
      view,
      mode: "fresh",
      anchor: opts.anchor ?? null,
      locate: opts.locate ?? null,
      scrollTop: opts.scrollTop ?? null,
      followScroll: opts.fromChange === true,
      diffAgainstLast: opts.fromChange === true,
    };
    this.syncNav();
    session.noteDoc(path);
    return true;
  }

  async back(): Promise<void> {
    this.history.saveScroll(this.scrollTop);
    const entry = this.history.back();
    if (!entry) return;
    this.syncNav();
    const ok = await this.open(entry.path, { history: "none", scrollTop: entry.scrollTop });
    if (!ok) {
      this.history.forward();
      this.syncNav();
    }
  }

  async forward(): Promise<void> {
    this.history.saveScroll(this.scrollTop);
    const entry = this.history.forward();
    if (!entry) return;
    this.syncNav();
    const ok = await this.open(entry.path, { history: "none", scrollTop: entry.scrollTop });
    if (!ok) {
      this.history.back();
      this.syncNav();
    }
  }

  /** Dismisses the "deleted" notice: back to the previous document, or an empty view. */
  async closeDeleted(): Promise<void> {
    if (this.canBack) {
      await this.back();
      return;
    }
    this.history.clear();
    this.current = null;
    this.request = null;
    this.deleted = null;
    this.error = null;
    this.syncNav();
    session.noteDoc(null);
  }

  /** Scroll within the current render to an anchor (an in-page link) or a located section. */
  jumpTo(anchor: string | null, locate: LocateRequest | null = null): void {
    this.jump = { n: ++this.jumpCount, anchor, locate };
  }

  noteScroll(top: number): void {
    this.scrollTop = top;
    this.scrolled = top > 4;
    session.noteScroll(top);
  }

  reportRender(marks: { changed: number; removed: number }): void {
    this.marks = marks;
  }

  // ---- live updates -----------------------------------------------------------------------------

  /** Looks at a watcher batch and refreshes or tombstones the open document if it is affected. */
  handleBatch(batch: FsBatch, followScroll: boolean): void {
    const path = this.current?.path;
    if (!path) return;
    const mine = batch.changes.filter((c) => c.path === path || c.from === path);

    const movedAway = mine.find((c) => c.kind === "renamed" && c.from === path);
    if (movedAway) {
      this.markDeleted(movedAway.path);
      return;
    }
    const last = mine.reduce<FsChange | null>(
      (best, c) => (best === null || c.atMs >= best.atMs ? c : best),
      null,
    );
    if (last && last.kind === "removed" && last.path === path) {
      this.scheduleDeleteCheck(path);
      return;
    }
    if (mine.length > 0 || batch.bulk !== null || batch.rescan) {
      this.cancelDeleteCheck();
      void this.refresh(followScroll);
    }
  }

  /** Re-reads the open document and renders it as an update (block diff, scroll anchoring). */
  async refresh(followScroll = false): Promise<void> {
    if (this.refreshing) {
      this.refreshQueued = true;
      this.queuedFollowScroll ||= followScroll;
      return;
    }
    const path = this.current?.path;
    if (!path) return;
    this.refreshing = true;
    const token = this.token;
    try {
      const view = await api.readDocument(path);
      if (token !== this.token || this.current?.path !== path) return;
      this.applyUpdate(view, followScroll);
    } catch {
      // the file may be mid-save or gone; a removed event schedules the delete check
    } finally {
      this.refreshing = false;
      if (this.refreshQueued) {
        const again = this.queuedFollowScroll;
        this.refreshQueued = false;
        this.queuedFollowScroll = false;
        void this.refresh(again);
      }
    }
  }

  private applyUpdate(view: DocumentView, followScroll: boolean): void {
    void tags.refreshMeta();
    this.current = view;
    this.deleted = null;
    this.request = {
      rev: ++this.rev,
      view,
      mode: "update",
      anchor: null,
      locate: null,
      scrollTop: null,
      followScroll,
      diffAgainstLast: false,
    };
  }

  private scheduleDeleteCheck(path: string): void {
    this.cancelDeleteCheck();
    this.deleteTimer = setTimeout(async () => {
      this.deleteTimer = null;
      if (this.current?.path !== path) return;
      try {
        const view = await api.readDocument(path);
        if (this.current?.path === path) this.applyUpdate(view, false);
      } catch {
        if (this.current?.path === path) this.markDeleted(null);
      }
    }, DELETE_GRACE_MS);
  }

  private cancelDeleteCheck(): void {
    if (this.deleteTimer !== null) clearTimeout(this.deleteTimer);
    this.deleteTimer = null;
  }

  private markDeleted(movedTo: string | null): void {
    this.cancelDeleteCheck();
    this.deleted = { atMs: Date.now(), movedTo };
  }

  // ---- loading indicator ------------------------------------------------------------------------

  private beginLoading(): void {
    this.loading = true;
    if (this.loadingTimer !== null) clearTimeout(this.loadingTimer);
    this.loadingTimer = setTimeout(() => {
      this.loadingVisible = this.loading;
    }, LOADING_DELAY_MS);
  }

  private endLoading(): void {
    this.loading = false;
    this.loadingVisible = false;
    if (this.loadingTimer !== null) clearTimeout(this.loadingTimer);
    this.loadingTimer = null;
  }

  private syncNav(): void {
    this.canBack = this.history.canBack;
    this.canForward = this.history.canForward;
  }
}

export const docs = new DocStore();
