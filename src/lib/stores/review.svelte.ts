// The review of what changed since a document was last approved (docs/design/review-changes-ui.md).
// Level 1 is quiet: a bar on a document that changed, and a layer only while the person is reviewing.

import { api } from "../api";
import { errorKind, errorMessage } from "../errors";
import { t, tn } from "../i18n";
import { basename } from "../paths";
import { indexAfterAction, recoverIndex, touchLru } from "../reviewNav";
import type { MessageKey } from "../i18n";
import type { ReviewBasis, ReviewChangeKind, ReviewEntry, ReviewStatus, ReviewView } from "../types";
import { toast } from "./toast.svelte";

/** How long a burst of writes waits before the review is asked again. */
export const REVIEW_DELAY_MS = 300;
/** Documents kept reviewable while the person follows a link and comes back. */
export const REVIEW_LRU = 5;
const DONE_MS = 2000;
const ANNOUNCE_MS = 5000;

export type ReviewPhase = "hidden" | "offer" | "reviewing" | "done";

class ReviewStore {
  view = $state.raw<ReviewView | null>(null);
  open = $state(false);
  current = $state(0);
  /** The file hash the offer bar was hidden for. A different hash, or opening the document again, shows it. */
  dismissedHash = $state<string | null>(null);
  busy = $state(false);
  entries = $state.raw<ReviewEntry[]>([]);
  /** Removals the person opened, by the start of their old range. */
  expanded = $state.raw<ReadonlySet<number>>(new Set());
  /** "All changes reviewed", until this time (ms). 0 when the flash is off. */
  doneUntil = $state(0);
  /** Quiet is level 1. Gate (level 2) is reserved: the bar is not dismissible and also covers new documents. */
  mode = $state<"quiet" | "gate">("quiet");
  /** A sentence for the bar's live region. */
  announcement = $state("");
  /** Bumped when the current change should be scrolled into view. A refresh does not bump it. */
  scrollNonce = $state(0);

  private timer: ReturnType<typeof setTimeout> | null = null;
  private lru: string[] = [];
  private undoDepth = 0;
  private pending: Promise<void> | null = null;
  /** The old-text position the current change had, so a refresh can find it again. */
  private anchorByte: number | null = null;
  private announcedAt = 0;
  private path = "";

  /** The documents the backend says differ from what was approved. */
  setEntries(entries: ReviewEntry[]): void {
    this.entries = entries;
    if (this.path !== "" && entries.some((entry) => entry.path === this.path) && this.view?.path !== this.path) {
      this.noteDocument(this.path, { ask: true, fresh: false });
    }
  }

  statusFor(path: string): ReviewStatus | null {
    if (this.view?.path === path) return this.view.status;
    return null;
  }

  /**
   * A document was rendered. Asks for its review once the burst settles, and keeps it among the few whose
   * review stays open across a link.
   */
  noteDocument(path: string, options: { ask: boolean; fresh: boolean }): void {
    if (options.fresh && path !== this.path) {
      this.path = path;
      this.dismissedHash = null;
      this.expanded = new Set();
      this.open = false;
      this.doneUntil = 0;
    }
    this.path = path;
    this.remember(path);
    if (!options.ask) return;
    if (this.mode === "quiet" && !this.entries.some((entry) => entry.path === path)) return;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.refresh(path);
    }, REVIEW_DELAY_MS);
  }

  /** Closes the layer and goes back to the offer bar when changes remain. Does not approve anything. */
  closeLayer(): void {
    this.open = false;
    this.announcement = "";
  }

  /** Hides the offer bar until the file changes or the document is opened again. */
  dismiss(): void {
    this.dismissedHash = this.view?.hash ?? "";
    this.open = false;
  }

  start(): void {
    if (!this.view || this.view.changes.length === 0) return;
    this.open = true;
    this.current = 0;
    this.anchorByte = this.view.changes[0]?.old.startByte ?? null;
    this.scrollNonce++;
    this.announce(
      tn("review.sr.opened", this.view.changes.length, {
        kind: t(kindWord(this.view.changes[0].kind)),
      }),
    );
  }

  move(direction: 1 | -1): void {
    const count = this.view?.changes.length ?? 0;
    if (!this.open || count === 0) return;
    this.current = (this.current + direction + count) % count;
    this.anchorByte = this.view?.changes[this.current]?.old.startByte ?? null;
    this.scrollNonce++;
    this.announce(this.at());
  }

  go(index: number): void {
    if (!this.view || index < 0 || index >= this.view.changes.length) return;
    this.current = index;
    this.anchorByte = this.view.changes[index]?.old.startByte ?? null;
    this.scrollNonce++;
  }

  toggleRemoved(startByte: number, open: boolean): void {
    const next = new Set(this.expanded);
    if (open) next.add(startByte);
    else next.delete(startByte);
    this.expanded = next;
  }

  async keep(index = this.current): Promise<void> {
    await this.act("keep", index);
  }

  async revert(index = this.current): Promise<void> {
    await this.act("revert", index);
  }

  async keepAll(): Promise<void> {
    await this.act("keepAll", null);
  }

  async revertAll(): Promise<void> {
    await this.act("revertAll", null);
  }

  async undo(): Promise<void> {
    const path = this.view?.path ?? this.path;
    if (!path || this.undoDepth === 0 || this.busy) return;
    await this.wait();
    this.busy = true;
    try {
      const next = await api.reviewUndo(path);
      this.undoDepth = Math.max(0, this.undoDepth - 1);
      this.take(next, this.anchorByte);
      toast.message(t("review.undone"));
      this.announce(tn("review.sr.undone", next.changes.length));
    } catch (error) {
      if (errorKind(error) === "reviewStale") {
        this.undoDepth = Math.max(0, this.undoDepth - 1);
        toast.message(t("review.undoStale", { name: basename(path) }), "warn");
        await this.refresh(path);
      } else {
        toast.message(errorMessage(error), "warn");
      }
    } finally {
      this.busy = false;
    }
  }

  async restoreDeleted(): Promise<void> {
    const view = this.view;
    if (!view) return;
    await this.wait();
    this.busy = true;
    try {
      await api.reviewRestoreDeleted(view.path, basis(view));
      toast.message(t("review.restored", { name: basename(view.path) }));
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    } finally {
      this.busy = false;
    }
  }

  /** Drops what the window shows, without telling the backend. The project is already gone. */
  clear(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.lru = [];
    this.view = null;
    this.open = false;
    this.undoDepth = 0;
    this.entries = [];
    this.path = "";
    this.scrollNonce = 0;
    this.dismissedHash = null;
    this.doneUntil = 0;
    this.announcement = "";
  }

  /** Lets go of every review this window was holding. The project is closing or being replaced. */
  async releaseAll(): Promise<void> {
    const paths = this.lru;
    this.clear();
    await Promise.all(paths.map((path) => api.reviewRelease(path).catch(() => undefined)));
  }

  private async act(kind: "keep" | "revert" | "keepAll" | "revertAll", index: number | null): Promise<void> {
    const view = this.view;
    if (!view || this.busy) return;
    await this.wait();
    const acted = index ?? this.current;
    this.anchorByte = view.changes[acted]?.old.startByte ?? this.anchorByte;
    this.busy = true;
    try {
      const basisNow = basis(view);
      const next =
        kind === "keep"
          ? await api.reviewKeep(view.path, basisNow, acted)
          : kind === "revert"
            ? await api.reviewRevert(view.path, basisNow, acted)
            : kind === "keepAll"
              ? await api.reviewKeepAll(view.path, basisNow)
              : await api.reviewRevertAll(view.path, basisNow);
      this.undoDepth++;
      const name = basename(view.path);
      const text =
        kind === "keep"
          ? t("review.kept")
          : kind === "revert"
            ? t("review.reverted", { name })
            : kind === "keepAll"
              ? t("review.keptAll")
              : t("review.revertedAll", { name });
      toast.message(text, "info", { label: t("review.undo"), run: () => void this.undo() });
      const landed = index === null ? null : indexAfterAction(view.changes.length, acted);
      this.take(next, this.anchorByte, landed);
      if (landed !== null) this.scrollNonce++;
      if (kind === "keep" || kind === "revert") {
        this.announce(
          tn(kind === "keep" ? "review.sr.kept" : "review.sr.reverted", next.changes.length),
        );
      }
    } catch (error) {
      const errorKindName = errorKind(error);
      if (errorKindName === "reviewStale") {
        toast.message(t("review.stale", { name: basename(view.path) }), "warn");
        await this.refresh(view.path);
      } else if (errorKindName === "reviewConflict") {
        toast.message(errorMessage(error), "warn");
      } else {
        toast.message(t("review.failed", { name: basename(view.path), message: errorMessage(error) }), "warn");
      }
    } finally {
      this.busy = false;
    }
  }

  private async refresh(path: string): Promise<void> {
    const job = (async () => {
      try {
        const next = await api.reviewGet(path);
        if (this.path !== path) return;
        const wasOpen = this.open;
        this.take(next, this.anchorByte);
        if (wasOpen && next.hash !== null) {
          const now = Date.now();
          if (now - this.announcedAt >= ANNOUNCE_MS) {
            this.announcedAt = now;
            this.announce(tn("review.sr.updated", next.changes.length));
          }
        }
      } catch {
        if (this.path === path && !this.open) this.view = null;
      }
    })();
    this.pending = job;
    await job;
    if (this.pending === job) this.pending = null;
  }

  private async wait(): Promise<void> {
    if (this.pending) await this.pending;
  }

  private take(next: ReviewView, anchor: number | null, landed: number | null = null): void {
    const count = next.changes.length;
    if (landed !== null && count > 0) this.current = Math.min(landed, count - 1);
    else if (anchor !== null && count > 0) this.current = recoverIndex(next.changes, anchor);
    else this.current = Math.min(this.current, Math.max(0, count - 1));
    this.view = next;
    if (count === 0 && this.open) {
      this.open = false;
      this.doneUntil = Date.now() + DONE_MS;
      this.announce(t("review.allDone"));
    } else if (next.status !== "changes" && next.status !== "new") {
      this.open = false;
    }
  }

  private remember(path: string): void {
    const { kept, dropped } = touchLru(this.lru, path, REVIEW_LRU);
    this.lru = kept;
    if (this.mode === "gate") return;
    for (const gone of dropped) void api.reviewRelease(gone).catch(() => undefined);
  }

  private at(): string {
    const change = this.view?.changes[this.current];
    if (!this.view || !change) return "";
    return t("review.sr.at", {
      i: this.current + 1,
      n: this.view.changes.length,
      kind: t(kindWord(change.kind)),
    });
  }

  private announce(text: string): void {
    this.announcement = "";
    this.announcement = text;
  }
}

function kindWord(kind: ReviewChangeKind): MessageKey {
  if (kind === "added") return "review.kindWord.added";
  if (kind === "removed") return "review.kindWord.removed";
  return "review.kindWord.modified";
}

function basis(view: ReviewView): ReviewBasis {
  return { hash: view.hash, reviewedHash: view.reviewedHash };
}

export const review = new ReviewStore();
