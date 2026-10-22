// Review: what changed in the open document since the person last read it, shown in the page with Keep | Revert for
// each change. One small machine: `idle` (nothing to review) | `offer` (changes exist: the bar offers them) |
// `reviewing` (the page shows them). The backend owns the truth (copies, the diff, the surgical save). A Markdown
// document is reviewed by blocks (the page), code and plain text by lines (a code view).

import { untrack } from "svelte";
import type { Basis, DocumentView, Result, ReviewView } from "../../api";
import type { Reason, Requirement } from "../../core/conditions/available";
import { conditions } from "../../core/conditions/context.svelte";
import { reasonKey } from "../../core/conditions/guidance";
import { documents } from "../../core/doc/document.svelte";
import { t, tn } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { InFront, onScreen } from "../../core/inFront.svelte";
import { windowStore, type NoticeAction, type NoticeGone } from "../../core/window.svelte";
import { kindWord } from "./blocks";

export type ReviewUi =
  | { status: "idle" }
  | { status: "offer"; path: string; count: number }
  | { status: "reviewing"; view: ReviewView; at: number };

const MARKDOWN = /\.(md|markdown|mdown|mkd|mdx|mdc)$/i;

/** Comparing with the last commit needs git and a repository (`conditions.md`). */
export const COMMIT_BASIS: Requirement = {
  needs: [{ need: "gitAvailable" }, { need: "gitRepo" }],
  prefers: [],
};

/** Why the last commit cannot be compared with here, or null when it can. */
function commitBlocked(): Reason | null {
  const availability = conditions.available(COMMIT_BASIS);
  return availability.status === "unavailable" ? availability.reason : null;
}

/** The document in front is looked at (read) when it has been shown this long. */
export const SEEN_MS = 1000;

/** A quiet line that stays until the person dismisses it (the window's lasting notice); `onGone` hears how it went.
 *  Showing it returns a function that takes it back. */
export type ReviewNotice = {
  text: string;
  actions: readonly NoticeAction[];
  onGone: (how: NoticeGone) => void;
};

/** A review closed with changes left in it, while "Review again" is offered. */
type Closed = { path: string; at: number; takeBack: () => void };

/** What a document is reviewed by: blocks of a page, lines of a code view, or nothing. */
export type ReviewKind = "blocks" | "lines" | null;

/** The kind of change `i` in words (for screen readers). */
function kindAt(view: ReviewView, i: number): string {
  const change = view.changes[i];
  return change ? kindWord(change.kind) : "";
}

/** What the store needs of the page: the document shown (reactive), and to make the blocks again. */
export type PagePort = {
  shown(): { path: string | null; hash: string | null; kind: ReviewKind };
  repaint(): void;
};

const page: PagePort = {
  shown: () => {
    const path = documents.path;
    const view = documents.view;
    const mine = view !== null && view.path === path;
    return {
      path,
      hash: mine ? view.hash : null,
      kind: mine && path !== null ? kindOf(view, path) : null,
    };
  },
  repaint: () => documents.refreshBlocks(),
};

/** How a shown document is reviewed. */
export function kindOf(view: DocumentView, path: string): ReviewKind {
  if (view.format === "markdown" && MARKDOWN.test(path)) return "blocks";
  const body = view.body;
  // text and config sheets are lines; a file shown in parts is not compared
  if (body.kind === "sheet" || (body.kind === "text" && !body.partial)) return "lines";
  return null;
}

export class ReviewStore {
  state = $state.raw<ReviewUi>({ status: "idle" });
  /** What documents are compared with. */
  basis = $state<Basis>("read");
  /** Keeps and reverts of this review that Undo can take back. */
  undoable = $state(0);
  /** Why the last choice of what to compare with did not take (shown next to the choice), or null. */
  basisNote = $state<string | null>(null);
  /** The latest thing said for screen readers (a live region in the bar). */
  announcement = $state("");
  #seq = 0;
  /** The document an `offer` | `reviewing` belongs to (to release it when the person leaves). */
  #held: string | null = null;
  /** A review just closed with changes in it: still held while "Review again" is offered (a stray Esc loses
   *  nothing), released when that offer goes or the person leaves the document. */
  #closed: Closed | null = null;
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #say: (text: string) => void;
  readonly #page: PagePort;
  /** The window is in front and visible, and this view is its tab on screen: looking counts only then. */
  readonly #front: InFront;
  readonly #commitBlocked: () => Reason | null;
  readonly #notice: (notice: ReviewNotice) => () => void;

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    say: (text: string) => void = (text) => windowStore.notifyNote(text),
    port: PagePort = page,
    shown: () => boolean = onScreen,
    commit: () => Reason | null = commitBlocked,
    notice: (notice: ReviewNotice) => () => void = (notice) => {
      const id = windowStore.notifyLasting(notice);
      return () => windowStore.dismiss(id);
    },
  ) {
    this.#client = client;
    this.#root = root;
    this.#say = say;
    this.#page = port;
    this.#front = new InFront(shown);
    this.#commitBlocked = commit;
    this.#notice = notice;
  }

  /** Why "the last commit" cannot be chosen here (no git, no repository), in one sentence; null when it can. */
  get commitUnavailable(): string | null {
    const reason = this.#commitBlocked();
    return reason === null ? null : t(reasonKey(reason));
  }

  get reviewing(): ReviewView | null {
    return this.state.status === "reviewing" ? this.state.view : null;
  }

  /** The review on screen, read again (a call that awaited may have opened one). */
  #started(): ReviewView | null {
    return this.reviewing;
  }

  /** The change the keys act on (index), or -1. */
  get current(): number {
    return this.state.status === "reviewing" ? this.state.at : -1;
  }

  /** Follows the document of the window until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) return;
        void this.#client.call("review.basis", {}).then((r) => {
          if (r.ok) this.basis = r.value;
        });
      });
      // the document shown (or its version) changed
      $effect(() => {
        const { path, hash, kind } = this.#page.shown();
        untrack(() => void this.#sync(kind === null ? null : path, hash));
      });
      // a document that is not Markdown, looked at for a moment, is the version the person last saw
      $effect(() => {
        const { path, hash, kind } = this.#page.shown();
        if (kind !== "lines" || path === null || hash === null || !this.#front.value) return;
        if (this.#root() === null) return;
        const timer = setTimeout(
          () => void this.#client.call("review.seen", { path, hash }),
          SEEN_MS,
        );
        return () => clearTimeout(timer);
      });
      $effect(() => this.#front.start());
      return () => this.#reset();
    });
  }

  #reset(): void {
    this.#seq += 1;
    this.#held = null;
    this.#forget();
    this.state = { status: "idle" };
    this.undoable = 0;
    this.basisNote = null;
  }

  async #sync(path: string | null, hash: string | null): Promise<void> {
    const seq = ++this.#seq;
    const reviewing = this.reviewing;
    if (this.#held !== null && this.#held !== path) {
      // the person left the document: its review is over
      void this.#client.call("review.release", { path: this.#held });
      this.#held = null;
      this.undoable = 0;
    }
    if (this.#closed !== null && this.#closed.path !== path) this.#letGo();
    if (path === null || hash === null) {
      this.state = { status: "idle" };
      this.#page.repaint();
      return;
    }
    if (reviewing !== null && reviewing.path === path) {
      if (reviewing.hash === hash) return;
      // the file changed under an open review: look again, keep the place
      const at = this.current;
      const result = await this.#client.call("review.open", { path, basis: this.basis });
      if (seq !== this.#seq) return;
      if (result.ok && result.value.count > 0) {
        this.state = {
          status: "reviewing",
          view: result.value,
          at: Math.min(at, result.value.count - 1),
        };
        this.announce(tn("review.sr.updated", result.value.count));
      } else {
        this.state = { status: "idle" };
        this.#held = null;
      }
      this.#page.repaint();
      return;
    }
    const status = await this.#client.call("review.status", { path });
    if (seq !== this.#seq) return;
    if (status.ok && status.value.state.state === "changes") {
      this.#held = path;
      // the bar offers them again: "Review again" is no longer what holds them
      if (this.#closed?.path === path) this.#forget();
      this.state = { status: "offer", path, count: status.value.state.count };
    } else {
      this.state = { status: "idle" };
    }
    if (status.ok) this.basis = status.value.basis;
    this.#page.repaint();
  }

  announce(text: string): void {
    this.announcement = text;
  }

  /** Shows the changes in the page. */
  async begin(): Promise<void> {
    const { path, kind } = this.#page.shown();
    if (path === null || kind === null) return;
    // asked again while "Review again" is offered (palette, key, F7): the same as that button
    const closed = this.#closed;
    if (closed !== null && closed.path === path) return this.#again(closed);
    const status = await this.#client.call("review.status", { path });
    if (!status.ok) return;
    let state = status.value.state;
    // nothing new since the person looked: what changed since the last commit is still worth a look
    let basis: Basis | undefined;
    if (
      status.value.basis === "read" &&
      (state.state === "same" || state.state === "noBasis") &&
      this.#commitBlocked() === null
    ) {
      const commit = await this.#client.call("review.status", { path, basis: "commit" });
      if (commit.ok && commit.value.state.state === "changes") {
        basis = "commit";
        state = commit.value.state;
        this.#say(t("review.fromCommit"));
      }
    }
    if (state.state === "tooLarge") return this.#say(t("review.tooLarge"));
    if (state.state === "noBasis") return this.#say(t("review.noBaseline"));
    if (state.state === "same") return this.#say(t("review.allDone"));
    if (state.state === "unsupported") return;
    const result = await this.#client.call("review.open", { path, basis });
    if (!result.ok) return;
    if (basis !== undefined) this.basis = basis;
    this.#held = path;
    this.undoable = 0;
    this.basisNote = null;
    this.#show(result.value, 0);
    this.announce(tn("review.sr.opened", result.value.count, { kind: kindAt(result.value, 0) }));
  }

  #show(view: ReviewView, at: number): void {
    this.state = {
      status: "reviewing",
      view,
      at: view.count === 0 ? -1 : Math.max(0, Math.min(at, view.count - 1)),
    };
    this.#page.repaint();
  }

  /** Closes the review: what is shown is what was read. Changes left in it stay held a moment, while a quiet line
   *  offers "Review again" at the same change (an Esc pressed by mistake loses nothing). */
  async done(): Promise<void> {
    const view = this.reviewing;
    const path = view?.path ?? this.#held;
    const at = this.current;
    this.#seq += 1;
    this.state = { status: "idle" };
    this.undoable = 0;
    this.#held = null;
    this.basisNote = null;
    this.#page.repaint();
    if (path === null) return;
    if (view !== null && view.count > 0) {
      if (this.#closed?.path === path) this.#forget();
      else this.#letGo();
      // no clock: it waits for the person (a keyboard user reaches it, or asks for the review again)
      const closed: Closed = { path, at, takeBack: () => undefined };
      this.#closed = closed;
      closed.takeBack = this.#notice({
        text: t("review.closed"),
        actions: [{ label: t("review.again"), run: () => void this.#again(closed) }],
        onGone: (how) => {
          if (how !== "action" && this.#closed === closed) this.#letGo();
        },
      });
      return;
    }
    await this.#client.call("review.release", { path });
  }

  /** The review that just closed opens again where it was. */
  async #again(closed: Closed): Promise<void> {
    if (this.#closed !== closed) return;
    this.#forget();
    const { path } = this.#page.shown();
    if (path !== closed.path) {
      void this.#client.call("review.release", { path: closed.path });
      return;
    }
    const result = await this.#client.call("review.open", { path, basis: this.basis });
    if (!result.ok || result.value.count === 0) {
      void this.#client.call("review.release", { path });
      if (result.ok) this.#say(t("review.allDone"));
      return;
    }
    this.#seq += 1;
    this.#held = path;
    this.undoable = 0;
    this.#show(result.value, closed.at);
    this.announce(
      t("review.sr.at", {
        i: this.current + 1,
        n: result.value.count,
        kind: kindAt(result.value, this.current),
      }),
    );
  }

  /** The review closed a moment ago is over: its changes are let go (they count as read). */
  #letGo(): void {
    const closed = this.#closed;
    if (closed === null) return;
    this.#forget();
    void this.#client.call("review.release", { path: closed.path });
  }

  /** "Review again" is no longer offered (its notice goes); what holds the changes is decided by the caller. */
  #forget(): void {
    const closed = this.#closed;
    if (closed === null) return;
    this.#closed = null;
    closed.takeBack();
  }

  async keep(index = this.current): Promise<void> {
    const view = this.reviewing;
    if (view === null || index < 0 || !view.canKeep) return;
    const result = await this.#client.call("review.keep", {
      path: view.path,
      hash: view.hash,
      index,
      basis: this.basis,
    });
    this.#after(result, index, "kept", true);
  }

  async revert(index = this.current): Promise<void> {
    const view = this.reviewing;
    if (view === null || index < 0) return;
    const result = await this.#client.call("review.revert", {
      path: view.path,
      hash: view.hash,
      index,
      basis: this.basis,
    });
    this.#after(result, index, "reverted", true);
  }

  async keepAll(): Promise<void> {
    const view = this.reviewing;
    if (view === null || !view.canKeep) return;
    const result = await this.#client.call("review.keepAll", {
      path: view.path,
      hash: view.hash,
      basis: this.basis,
    });
    this.#after(result, 0, "keptAll", false);
  }

  async revertAll(): Promise<void> {
    const view = this.reviewing;
    if (view === null) return;
    const result = await this.#client.call("review.revertAll", {
      path: view.path,
      hash: view.hash,
      basis: this.basis,
    });
    this.#after(result, 0, "revertedAll", false);
  }

  async undo(): Promise<void> {
    const view = this.reviewing;
    if (view === null || this.undoable === 0) return;
    const result = await this.#client.call("review.undo", { path: view.path, basis: this.basis });
    if (!result.ok) {
      this.#say(t("review.undoStale", { name: view.path.split("/").at(-1) ?? view.path }));
      return;
    }
    this.undoable -= 1;
    this.#show(result.value, this.current);
    this.announce(tn("review.sr.undone", result.value.count));
  }

  /** What a keep | revert answered: the review goes on, or ends when nothing is left. */
  #after(
    result: Result<ReviewView>,
    index: number,
    what: "kept" | "reverted" | "keptAll" | "revertedAll",
    single: boolean,
  ): void {
    const view = this.reviewing;
    const name = view?.path.split("/").at(-1) ?? "";
    if (!result.ok) {
      const error = result.error;
      if (error.kind === "backend" && error.error.kind === "conflict") {
        // the file is not the version the changes were drawn from: look again
        this.#say(t("review.stale", { name }));
        if (view !== null) void this.#refresh(view.path);
      } else {
        this.#say(
          t("review.failed", {
            name,
            message: error.kind === "backend" ? error.error.kind : error.message,
          }),
        );
      }
      return;
    }
    if (single) this.undoable += 1;
    else this.undoable = 1;
    const next = result.value;
    if (next.count === 0) {
      this.announce(t("review.allDone"));
      this.#say(
        t(what === "keptAll" || what === "kept" ? "review.keptAll" : "review.revertedAll", {
          name,
        }),
      );
      this.state = { status: "idle" };
      this.#held = null;
      this.undoable = 0;
      this.#page.repaint();
      return;
    }
    this.#show(next, single ? index : 0);
    this.announce(
      what === "kept"
        ? tn("review.sr.kept", next.count)
        : what === "reverted"
          ? tn("review.sr.reverted", next.count)
          : tn("review.sr.opened", next.count, { kind: kindAt(next, 0) }),
    );
  }

  async #refresh(path: string): Promise<void> {
    const result = await this.#client.call("review.open", { path });
    if (result.ok && result.value.count > 0) this.#show(result.value, this.current);
    else {
      this.state = { status: "idle" };
      this.#page.repaint();
    }
  }

  go(step: 1 | -1): void {
    const view = this.reviewing;
    if (view === null || view.count === 0) return;
    const at = (this.current + step + view.count) % view.count;
    this.state = { status: "reviewing", view, at };
    this.announce(
      t("review.sr.at", {
        i: at + 1,
        n: view.count,
        kind: kindAt(view, at),
      }),
    );
    this.#page.repaint();
  }

  /**
   * F7 | Shift+F7: the next | previous change. Reviewing -> the next | previous of the review (wraps); only an offer
   * for the shown document -> the review opens at its first (next) | last (previous) change. False when there is
   * nothing to move to.
   */
  async step(direction: 1 | -1): Promise<boolean> {
    const open = this.reviewing;
    if (open !== null) {
      if (open.count === 0) return false;
      this.go(direction);
      return true;
    }
    const { path, kind } = this.#page.shown();
    if (this.state.status !== "offer" || this.state.path !== path || kind === null) return false;
    await this.begin();
    const started = this.#started();
    if (started === null || started.count === 0) return false;
    if (direction === -1) this.go(-1);
    return true;
  }

  /** Compares with `basis` from now on. The last commit where it cannot be had (no git, no repository, no commit
   *  of the file): the choice stays as it was and a note next to it says why (`basisNote`), never a silent switch. */
  async setBasis(basis: Basis): Promise<void> {
    if (basis === this.basis) return;
    const blocked = basis === "commit" ? this.commitUnavailable : null;
    if (blocked !== null) {
      this.basisNote = blocked;
      return;
    }
    const before = this.basis;
    const view = this.reviewing;
    if (view !== null) {
      const result = await this.#client.call("review.open", { path: view.path, basis });
      if (!result.ok) {
        this.basisNote = t("review.basisFailed");
        return;
      }
      if (this.basis !== before) return;
      this.basisNote = null;
      this.basis = basis;
      await this.#client.call("review.setBasis", { basis });
      this.#show(result.value, 0);
      // the set of changes is another one now: said, not only drawn
      this.announce(
        tn(
          basis === "commit" ? "review.sr.basisCommit" : "review.sr.basisRead",
          result.value.count,
        ),
      );
      return;
    }
    this.basisNote = null;
    this.basis = basis;
    await this.#client.call("review.setBasis", { basis });
  }

  /** Writes a deleted document back as it was last read. */
  async restore(path: string): Promise<void> {
    const result = await this.#client.call("review.restore", { path });
    if (result.ok) this.#say(t("review.restored", { name: path.split("/").at(-1) ?? path }));
  }
}

export const review = new ReviewStore();
