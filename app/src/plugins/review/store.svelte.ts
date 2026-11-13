// Review: what changed in the open document since the person last read it, shown in the page with Keep | Revert for
// each change. One small machine: `idle` (nothing to review) | `offer` (changes exist: the bar offers them) |
// `reviewing` (the page shows them). The backend owns the truth (copies, the diff, the surgical save). A Markdown
// document is reviewed by blocks (the page), code and plain text by lines (a code view).
//
// Besides what was read and the last commit, a review can compare with an earlier version of the document: one of
// the commits that changed it, picked in a list the person asks for. A version belongs to the review it was picked
// for: it is never the choice the backend remembers, and it goes when that review ends.

import { untrack } from "svelte";
import type {
  Basis,
  DocumentView,
  Result,
  ReviewStatus,
  ReviewView,
  VersionGap,
  VersionRow,
  VersionsReason,
} from "../../api";
import type { Reason, Requirement } from "../../core/conditions/available";
import { READ_MS, personScrolls } from "../../core/readRule";
import { conditions } from "../../core/conditions/context.svelte";
import { reasonKey } from "../../core/conditions/guidance";
import { documents } from "../../core/doc/document.svelte";
import { editing } from "../../core/editing/session.svelte";
import { language, t, tn, type MessageKey } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { InFront, onScreen } from "../../core/inFront.svelte";
import { exactTime } from "../../core/time";
import { windowStore, type NoticeAction, type NoticeGone } from "../../core/window.svelte";
import { kindWord } from "./blocks";

export type ReviewUi =
  | { status: "idle" }
  | { status: "offer"; path: string; count: number }
  | { status: "reviewing"; view: ReviewView; at: number };

/** What the person can choose to compare with from now on (the backend remembers it). */
type Choice = Exclude<Basis, { version: string }>;

/** The earlier version a review compares with: its commit and when it was made. */
export type PickedVersion = { commit: string; atMs: number };

/** Why a document has no list of versions: what the backend said, text in the editor that is not saved, or no git
 *  here (`text`: which, in a sentence). */
export type VersionsProblem =
  { kind: VersionsReason } | { kind: "unsaved" } | { kind: "git"; text: string };

/** The earlier versions of the document shown, in the list the person asked for. */
export type VersionsUi =
  | { status: "closed" }
  | {
      status: "open";
      path: string;
      loading: boolean;
      /** Newest first, every page read so far. */
      rows: readonly VersionRow[];
      /** There is more to read (`moreVersions`); null at the end of the history. */
      next: number | null;
      /** The path the file had before its oldest version (those versions are not listed). */
      renamedFrom: string | null;
      /** The repository is a partial copy: older versions may be missing. */
      cut: boolean;
      /** Why there is no list, or why its next page did not come. */
      problem: VersionsProblem | null;
      /** One line about the list: how the last pick went, or that the history changed. */
      note: string | null;
    };

const PROBLEM_KEYS: Readonly<Record<VersionsReason | "unsaved", MessageKey>> = {
  unavailable: "review.versions.none.unavailable",
  otherRepository: "review.versions.none.otherRepository",
  slow: "review.versions.none.slow",
  failed: "review.versions.none.failed",
  notCommitted: "review.versions.none.notCommitted",
  onlyCurrent: "review.versions.none.onlyCurrent",
  unsupported: "review.versions.none.unsupported",
  unsaved: "review.versions.none.unsaved",
};

/** Why there is no list of versions, in one sentence. */
export function problemText(problem: VersionsProblem): string {
  return problem.kind === "git" ? problem.text : t(PROBLEM_KEYS[problem.kind]);
}

const GAP_KEYS: Readonly<Record<VersionGap, MessageKey>> = {
  missing: "review.versions.pick.missing",
  tooLarge: "review.versions.pick.tooLarge",
  notText: "review.versions.pick.notText",
};

/** Why a version asked for has nothing to review, in one sentence; null when it has changes. */
function refusal(status: ReviewStatus): string | null {
  const state = status.state;
  switch (state.state) {
    case "changes":
      return null;
    case "same":
      return t("review.versions.pick.same");
    case "noBasis":
      return t(status.gap === undefined ? "review.versions.pick.failed" : GAP_KEYS[status.gap]);
    case "tooLarge":
      return t("review.tooLarge");
    case "unsupported":
      return t("review.versions.none.unsupported");
    default:
      return state satisfies never;
  }
}

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

/** The document in front is looked at (read) when it has been shown this long, or sooner when the person scrolls it. */
export const SEEN_MS = READ_MS;

/** A quiet line that stays until the person dismisses it (the window's lasting notice); `onGone` hears how it went.
 *  Showing it returns a function that takes it back. */
export type ReviewNotice = {
  text: string;
  actions: readonly NoticeAction[];
  onGone: (how: NoticeGone) => void;
};

/** A review closed with changes left in it, while "Review again" is offered (`picked`: the version it compared
 *  with, so that it opens again on the same comparison). */
type Closed = { path: string; at: number; picked: PickedVersion | null; takeBack: () => void };

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
  /** The list of earlier versions the person asked for. */
  versions = $state.raw<VersionsUi>({ status: "closed" });
  /** The version the review on screen compares with (`basis` names it too), or null. */
  #picked = $state.raw<PickedVersion | null>(null);
  /** What was compared with before a version was picked: what comes back when its review ends. */
  #choice: Choice = "read";
  /** Reads of the list: only the newest answer is taken. */
  #listed = 0;
  /** The commit the repository is at, as last heard (`undefined`: not heard yet). */
  #head: string | null | undefined = undefined;
  /** Reads (status, open): only the newest answer is drawn. Writes go up too, so a read sent before a write never
   *  draws the file as it was over what the write answered. */
  #seq = 0;
  /** The review that keeps | reverts | undos belong to: goes up when a review opens or closes, or the person leaves
   *  its document. A write that came back for the same review on the same document is recorded, whatever ran since:
   *  the file changed, so Undo must be able to take it back. */
  #review = 0;
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
  /** The editor holds text that is not on disk: the versions would be compared with text the page does not show. */
  readonly #unsaved: () => boolean;

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
    unsaved: () => boolean = () => editing.unsaved,
  ) {
    this.#client = client;
    this.#root = root;
    this.#say = say;
    this.#page = port;
    this.#front = new InFront(shown);
    this.#commitBlocked = commit;
    this.#notice = notice;
    this.#unsaved = unsaved;
  }

  /** Why "the last commit" cannot be chosen here (no git, no repository), in one sentence; null when it can. */
  get commitUnavailable(): string | null {
    const reason = this.#commitBlocked();
    return reason === null ? null : t(reasonKey(reason));
  }

  /** The earlier version the review on screen compares with, or null. */
  get picked(): PickedVersion | null {
    return this.#picked;
  }

  get reviewing(): ReviewView | null {
    return this.state.status === "reviewing" ? this.state.view : null;
  }

  /** The review on screen, read again (a call that awaited may have opened one). */
  #started(): ReviewView | null {
    return this.reviewing;
  }

  /** The review on screen when it belongs to the document shown: keys and buttons act only on that file (while
   *  another document loads, the review of the one before is still on screen). */
  #mine(): ReviewView | null {
    const view = this.reviewing;
    return view !== null && view.path === this.#page.shown().path ? view : null;
  }

  /** An answer that came back is still wanted: nothing newer ran meanwhile, and `path` is still the document shown
   *  (the person may have moved on while the backend answered). */
  #still(seq: number, path: string): boolean {
    return seq === this.#seq && this.#page.shown().path === path;
  }

  /** What a write answered belongs to the review on screen: it is still open and its document is still shown. */
  #here(review: number, path: string): boolean {
    return review === this.#review && this.#page.shown().path === path;
  }

  /** An open of `path` came back too late. The person left: nothing stays held for it (the backend may have opened
   *  it after the release that went with leaving). Still on it: whatever ran since holds it. */
  #letGoIfLeft(path: string): void {
    if (this.#page.shown().path !== path) void this.#client.call("review.release", { path });
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
          if (r.ok) this.#chosen(r.value);
        });
      });
      // the repository as it is now: at another commit, the history a list | a picked version came from may be gone
      $effect(() => {
        if (this.#root() === null) return;
        let gone = false;
        let off: (() => void) | null = null;
        void this.#client
          .on("git:snapshot", (snapshot) => this.#moved(snapshot?.head.oid ?? null))
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        return () => {
          gone = true;
          off?.();
          this.#head = undefined;
        };
      });
      // the document shown (or its version) changed
      $effect(() => {
        const { path, hash, kind } = this.#page.shown();
        untrack(() => {
          // a list is of the document it was asked for
          if (this.versions.status === "open" && this.versions.path !== path) this.closeVersions();
          void this.#sync(kind === null ? null : path, hash);
        });
      });
      // a document that is not Markdown, looked at for a moment, is the version the person last saw
      $effect(() => {
        const { path, hash, kind } = this.#page.shown();
        if (kind !== "lines" || path === null || hash === null || !this.#front.value) return;
        if (this.#root() === null) return;
        let told = false;
        const seen = (): void => {
          if (told) return;
          told = true;
          void this.#client.call("review.seen", { path, hash });
        };
        const timer = setTimeout(seen, SEEN_MS);
        const offScroll = personScrolls(() => {
          if (this.#front.value) seen();
        });
        return () => {
          clearTimeout(timer);
          offScroll();
        };
      });
      $effect(() => this.#front.start());
      return () => this.#reset();
    });
  }

  #reset(): void {
    this.#seq += 1;
    this.#review += 1;
    this.#held = null;
    this.#forget();
    this.state = { status: "idle" };
    this.undoable = 0;
    this.basisNote = null;
    this.#over();
  }

  /** What the backend says the person chose to compare with (never a version: none is remembered). While a version
   *  is picked, it is what the review goes back to. */
  #chosen(basis: Basis): void {
    if (typeof basis !== "string") return;
    if (this.#picked === null) this.basis = basis;
    else this.#choice = basis;
  }

  /** The review on screen compares with `version` from now on; what was chosen before is kept to go back to. */
  #pick(version: PickedVersion): void {
    if (typeof this.basis === "string") this.#choice = this.basis;
    this.#picked = version;
    this.basis = { version: version.commit };
  }

  /** The picked version goes: what is compared with is the person's choice again. */
  #unpick(): void {
    if (this.#picked === null) return;
    this.#picked = null;
    this.basis = this.#choice;
  }

  /** The review on screen ended: the version picked for it goes with it, and so does the list. */
  #over(): void {
    this.#unpick();
    this.closeVersions();
  }

  async #sync(path: string | null, hash: string | null): Promise<void> {
    const seq = ++this.#seq;
    const reviewing = this.reviewing;
    if (this.#held !== null && this.#held !== path) {
      // the person left the document: its review is over
      void this.#client.call("review.release", { path: this.#held });
      this.#held = null;
      this.#review += 1;
      this.undoable = 0;
      this.#unpick();
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
        this.#over();
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
    if (status.ok) this.#chosen(status.value.basis);
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
    const seq = ++this.#seq;
    const status = await this.#client.call("review.status", { path });
    if (!status.ok || !this.#still(seq, path)) return;
    let state = status.value.state;
    // nothing new since the person looked: what changed since the last commit is still worth a look
    let basis: Basis | undefined;
    if (
      status.value.basis === "read" &&
      (state.state === "same" || state.state === "noBasis") &&
      this.#commitBlocked() === null
    ) {
      const commit = await this.#client.call("review.status", { path, basis: "commit" });
      if (!this.#still(seq, path)) return;
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
    if (!this.#still(seq, path)) return this.#letGoIfLeft(path);
    if (!result.ok) return;
    // opened on the person's choice: a version picked before is not what this review compares with
    this.#unpick();
    if (basis !== undefined) this.basis = basis;
    this.#fresh(path);
    this.basisNote = null;
    this.#show(result.value, 0);
    this.announce(tn("review.sr.opened", result.value.count, { kind: kindAt(result.value, 0) }));
  }

  /** A review of `path` starts: it holds the document, and Undo starts with nothing to take back. */
  #fresh(path: string): void {
    this.#held = path;
    this.#review += 1;
    this.undoable = 0;
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
    const picked = this.#picked;
    this.#seq += 1;
    this.#review += 1;
    this.state = { status: "idle" };
    this.undoable = 0;
    this.#held = null;
    this.basisNote = null;
    this.#over();
    this.#page.repaint();
    if (path === null) return;
    if (view !== null && view.count > 0) {
      if (this.#closed?.path === path) this.#forget();
      else this.#letGo();
      // no clock: it waits for the person (a keyboard user reaches it, or asks for the review again)
      const closed: Closed = { path, at, picked, takeBack: () => undefined };
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
    const seq = ++this.#seq;
    const basis: Basis = closed.picked === null ? this.basis : { version: closed.picked.commit };
    const result = await this.#client.call("review.open", { path, basis });
    if (!this.#still(seq, path)) return this.#letGoIfLeft(path);
    if (!result.ok || result.value.count === 0) {
      void this.#client.call("review.release", { path });
      if (result.ok) this.#say(t("review.allDone"));
      return;
    }
    this.#fresh(path);
    if (closed.picked !== null) this.#pick(closed.picked);
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

  /** A keep | revert | undo goes out: reads sent before it are superseded. Returns the review it belongs to. */
  #write(): number {
    this.#seq += 1;
    return this.#review;
  }

  async keep(index = this.current): Promise<void> {
    const view = this.#mine();
    if (view === null || index < 0 || !view.canKeep) return;
    const review = this.#write();
    const result = await this.#client.call("review.keep", {
      path: view.path,
      hash: view.hash,
      index,
      basis: this.basis,
    });
    this.#after(result, index, "kept", true, review, view.path);
  }

  async revert(index = this.current): Promise<void> {
    const view = this.#mine();
    if (view === null || index < 0) return;
    const review = this.#write();
    const result = await this.#client.call("review.revert", {
      path: view.path,
      hash: view.hash,
      index,
      basis: this.basis,
    });
    this.#after(result, index, "reverted", true, review, view.path);
  }

  async keepAll(): Promise<void> {
    const view = this.#mine();
    if (view === null || !view.canKeep) return;
    const review = this.#write();
    const result = await this.#client.call("review.keepAll", {
      path: view.path,
      hash: view.hash,
      basis: this.basis,
    });
    this.#after(result, 0, "keptAll", false, review, view.path);
  }

  async revertAll(): Promise<void> {
    const view = this.#mine();
    if (view === null) return;
    const review = this.#write();
    const result = await this.#client.call("review.revertAll", {
      path: view.path,
      hash: view.hash,
      basis: this.basis,
    });
    this.#after(result, 0, "revertedAll", false, review, view.path);
  }

  async undo(): Promise<void> {
    const view = this.#mine();
    if (view === null || this.undoable === 0) return;
    const review = this.#write();
    const result = await this.#client.call("review.undo", { path: view.path, basis: this.basis });
    if (!this.#here(review, view.path)) return;
    if (!result.ok) {
      this.#say(t("review.undoStale", { name: view.path.split("/").at(-1) ?? view.path }));
      return;
    }
    // two undos sent before the first answer: the count never goes below none
    this.undoable = Math.max(0, this.undoable - 1);
    this.#show(result.value, this.current);
    this.announce(tn("review.sr.undone", result.value.count));
  }

  /** What a keep | revert answered: the review goes on, or ends when nothing is left. */
  #after(
    result: Result<ReviewView>,
    index: number,
    what: "kept" | "reverted" | "keptAll" | "revertedAll",
    single: boolean,
    review: number,
    path: string,
  ): void {
    // the person moved on | closed the review: what the file became is drawn when it is shown again
    if (!this.#here(review, path)) return;
    const name = path.split("/").at(-1) ?? "";
    if (!result.ok) {
      const error = result.error;
      if (error.kind === "backend" && error.error.kind === "conflict") {
        // the file is not the version the changes were drawn from: look again
        this.#say(t("review.stale", { name }));
        void this.#refresh(path);
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
      this.#review += 1;
      this.undoable = 0;
      this.#over();
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
    const seq = ++this.#seq;
    // against what the review on screen compares with (a version is not what the backend remembers)
    const result = await this.#client.call("review.open", { path, basis: this.basis });
    if (!this.#still(seq, path)) return this.#letGoIfLeft(path);
    if (result.ok && result.value.count > 0) this.#show(result.value, this.current);
    else {
      this.state = { status: "idle" };
      this.#over();
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
   *  of the file): the choice stays as it was and a note next to it says why (`basisNote`), never a silent switch.
   *  A version is no choice to remember: it is picked in the list (`pickVersion`), and going back to what was read
   *  or the last commit here lets it go. */
  async setBasis(basis: Basis): Promise<void> {
    if (typeof basis !== "string" || basis === this.basis) return;
    const blocked = basis === "commit" ? this.commitUnavailable : null;
    if (blocked !== null) {
      this.basisNote = blocked;
      return;
    }
    const before = this.basis;
    const view = this.#mine();
    if (view !== null) {
      const seq = ++this.#seq;
      const result = await this.#client.call("review.open", { path: view.path, basis });
      if (!this.#still(seq, view.path)) return this.#letGoIfLeft(view.path);
      if (!result.ok) {
        this.basisNote = t("review.basisFailed");
        return;
      }
      if (this.basis !== before) return;
      await this.#client.call("review.setBasis", { basis });
      if (!this.#still(seq, view.path)) return this.#letGoIfLeft(view.path);
      // the choice shows only with the review drawn on it
      this.basisNote = null;
      this.#picked = null;
      this.basis = basis;
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
    this.#picked = null;
    this.basis = basis;
    await this.#client.call("review.setBasis", { basis });
  }

  /**
   * Opens the list of earlier versions of the document shown. No git here, or text in the editor that is not
   * saved: the list says so and nothing is read (the versions would be compared with text the page does not show).
   */
  async openVersions(): Promise<void> {
    const { path } = this.#page.shown();
    if (path !== null) await this.#list(path, null);
  }

  /** Reads the next page of the list (`next`), added after the rows shown. */
  async moreVersions(): Promise<void> {
    const list = this.versions;
    if (list.status !== "open" || list.loading || list.next === null) return;
    await this.#read(list.path, list.next, null);
  }

  /** Reads again what did not come (too slow, failed): the next page when rows are shown, else the list. */
  async retryVersions(): Promise<void> {
    const list = this.versions;
    if (list.status !== "open" || list.loading) return;
    if (list.next !== null) await this.#read(list.path, list.next, null);
    else await this.#list(list.path, null);
  }

  /** Closes the list. A page on its way is dropped. */
  closeVersions(): void {
    this.#listed += 1;
    if (this.versions.status === "open") this.versions = { status: "closed" };
  }

  /** The list of `path` from its first page, with `note` on it; or why it cannot be read here. */
  async #list(path: string, note: string | null): Promise<void> {
    const git = this.commitUnavailable;
    const problem: VersionsProblem | null =
      git !== null ? { kind: "git", text: git } : this.#unsaved() ? { kind: "unsaved" } : null;
    if (problem === null) return this.#read(path, null, note);
    this.#listed += 1;
    this.versions = {
      status: "open",
      path,
      loading: false,
      rows: [],
      next: null,
      renamedFrom: null,
      cut: false,
      problem,
      note,
    };
  }

  /** Reads one page of the list of `path`: the first one (`after` null) replaces the rows, a later one is added. */
  async #read(path: string, after: number | null, note: string | null): Promise<void> {
    const asked = ++this.#listed;
    const before = this.versions;
    // the rows shown stay while the page is on its way
    const shown = before.status === "open" && before.path === path ? before : null;
    this.versions = {
      status: "open",
      path,
      rows: shown?.rows ?? [],
      next: shown?.next ?? null,
      renamedFrom: shown?.renamedFrom ?? null,
      cut: shown?.cut ?? false,
      loading: true,
      problem: null,
      note,
    };
    if (this.#head === undefined) void this.#learnHead();
    const result = await this.#client.call(
      "review.versions",
      after === null ? { path } : { path, after },
    );
    const list = this.versions;
    // the person closed the list, asked again, or moved to another document meanwhile
    if (asked !== this.#listed || list.status !== "open" || this.#page.shown().path !== path) {
      return;
    }
    if (!result.ok || result.value.state === "unavailable") {
      const kind =
        result.ok && result.value.state === "unavailable" ? result.value.reason : "failed";
      this.versions = { ...list, loading: false, problem: { kind } };
      return;
    }
    const page = result.value;
    this.versions = {
      ...list,
      loading: false,
      rows: after === null ? page.rows : [...list.rows, ...page.rows],
      next: page.next,
      renamedFrom: page.renamedFrom,
      cut: page.cut,
    };
  }

  /** One line on the open list (how a pick went). */
  #tell(note: string): void {
    const list = this.versions;
    if (list.status === "open") this.versions = { ...list, note };
  }

  /**
   * Compares the document shown with the version of `row`, picked in the list. It has changes: the review opens
   * against it (an open review is drawn again on it) and the list closes. The same text, or a version that cannot
   * be compared: the list stays, with a line that says so; nothing else is compared with instead.
   */
  async pickVersion(row: VersionRow): Promise<void> {
    const list = this.versions;
    const { path } = this.#page.shown();
    if (list.status !== "open" || path === null || list.path !== path) return;
    const basis: Basis = { version: row.commit };
    const seq = ++this.#seq;
    const status = await this.#client.call("review.status", { path, basis });
    if (!this.#still(seq, path)) return;
    const why = status.ok ? refusal(status.value) : t("review.versions.pick.failed");
    if (why !== null) return this.#tell(why);
    const result = await this.#client.call("review.open", { path, basis });
    if (!this.#still(seq, path)) return this.#letGoIfLeft(path);
    if (!result.ok) return this.#tell(t("review.versions.pick.failed"));
    if (this.#mine() === null) {
      // no review was open: this one starts, and "Review again" is no longer what holds the changes
      if (this.#closed?.path === path) this.#forget();
      this.#fresh(path);
    }
    this.basisNote = null;
    this.#pick({ commit: row.commit, atMs: row.atMs });
    this.closeVersions();
    this.#show(result.value, 0);
    this.announce(
      tn("review.sr.basisVersion", result.value.count, {
        when: exactTime(row.atMs, language.locale),
      }),
    );
  }

  /** Asks where the repository is, so that a later move is told from what a list was read at. */
  async #learnHead(): Promise<void> {
    const result = await this.#client.call("git.snapshot", {});
    if (result.ok && this.#head === undefined) this.#head = result.value?.head.oid ?? null;
  }

  /** The repository was announced at `head`. At another commit than before, the history may be another one: an open
   *  list is read again from its first page, and a picked version is asked for again. */
  #moved(head: string | null): void {
    const before = this.#head;
    this.#head = head;
    if (before === undefined || before === head) return;
    const list = this.versions;
    if (list.status === "open") void this.#list(list.path, t("review.versions.reread"));
    if (this.#picked !== null) void this.#recheck();
  }

  /** The history moved under a review against a version. The version is gone: the review goes back to the last
   *  commit, and a note next to the choice says so. */
  async #recheck(): Promise<void> {
    const picked = this.#picked;
    const path = this.#mine()?.path;
    if (picked === null || path === undefined) return;
    // no read of the review is superseded by this question: it draws nothing
    const status = await this.#client.call("review.status", {
      path,
      basis: { version: picked.commit },
    });
    if (this.#picked !== picked || this.#page.shown().path !== path) return;
    if (!status.ok || status.value.state.state !== "noBasis") return;
    await this.setBasis("commit");
    if (this.#picked === null && this.basis === "commit" && this.#mine()?.path === path) {
      this.basisNote = t("review.versions.gone");
    }
  }

  /** Writes a deleted document back as it was last read. */
  async restore(path: string): Promise<void> {
    const result = await this.#client.call("review.restore", { path });
    if (result.ok) this.#say(t("review.restored", { name: path.split("/").at(-1) ?? path }));
  }
}

export const review = new ReviewStore();
