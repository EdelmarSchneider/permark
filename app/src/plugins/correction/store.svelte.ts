// Correction review of one window (spec feature-correction D-7, §4.0). A run: pieces of the editor's text in the
// scope (`segment.ts`, offsets = the editor's text, LF) go to `correction.run`; news `correction:run` of this run only;
// each answer -> markers back + guards (`guards.ts`) -> suggestions (document offsets) merged in document order;
// discarded answers only counted. Offsets follow the text: own edits shift what follows exactly; any other change
// (typing, the other editor) -> prefix/suffix diff, suggestions | pieces touching it dropped (counted), the rest shifted.
// Accept = one checked `session.apply` (expect = original): refused while composing -> retried shortly; refused else
// -> dropped (counted). Accept all = one apply (one undo step); a batch refused (Edit mode: a locked block, …) ->
// each edit alone, only the refused ones dropped. File switch | conflict | View -> review closed.

import { untrack } from "svelte";
import { api as defaultApi, type Api, type ApiError, type Unlisten } from "../../api";
import type { CorrectionPieceResult } from "../../api/generated/CorrectionPieceResult";
import type { CorrectionRun } from "../../api/generated/CorrectionRun";
import {
  editing,
  type EditMode,
  type EditScope,
  type EditingSession,
  type TextEdit,
  type TextRange,
} from "../../core/editing/session.svelte";
import { around, type Suggestion } from "./diff";
import { judge } from "./guards";
import { pieces, type Piece } from "./segment";

/** What the store needs of the editing session. */
export type Session = Pick<
  EditingSession,
  | "path"
  | "mode"
  | "state"
  | "text"
  | "current"
  | "scope"
  | "apply"
  | "reveal"
  | "composing"
  | "focus"
>;

/** A suggestion with its identity (offsets change, the key stays). */
export type Found = Suggestion & { key: number };

export type RunStatus =
  | { kind: "loading"; progress: number }
  | { kind: "checking"; part: number; parts: number }
  | { kind: "finished" }
  | { kind: "cancelled" }
  | { kind: "failed"; error: ApiError };

export type Review = {
  status: RunStatus;
  /** In document order. */
  suggestions: readonly Found[];
  /** Index of the current suggestion (0 when none). */
  at: number;
  /** Pieces left as they were (the model changed too much | the piece was too long). */
  discarded: number;
  /** Suggestions dropped because the text changed under them. */
  outdated: number;
};

export type CorrectionState =
  { kind: "closed" } | { kind: "nothing" } | ({ kind: "review" } & Review);

/** Wait between tries of an edit refused while text is composed (ms), and how many tries. */
const COMPOSE_RETRY_MS = 100;
const COMPOSE_TRIES = 50;

const CLOSED: CorrectionState = { kind: "closed" };

export function running(status: RunStatus): boolean {
  return status.kind === "loading" || status.kind === "checking";
}

/** The range of `before` that differs from `after` (`to`: end in `before`) and the length change. */
export function changedRange(before: string, after: string): TextRange & { delta: number } {
  const max = Math.min(before.length, after.length);
  let head = 0;
  while (head < max && before.charCodeAt(head) === after.charCodeAt(head)) head += 1;
  let tail = 0;
  while (
    tail < max - head &&
    before.charCodeAt(before.length - 1 - tail) === after.charCodeAt(after.length - 1 - tail)
  ) {
    tail += 1;
  }
  return { from: head, to: before.length - tail, delta: after.length - before.length };
}

function newRunId(): string {
  return globalThis.crypto.randomUUID();
}

export class CorrectionStore {
  state = $state.raw<CorrectionState>(CLOSED);
  /** A model is assigned to job "correction" (dormant until then). */
  available = $state(false);
  /** Bumped when the keyboard should go to the bar's first action (Accept). */
  focusRequest = $state(0);

  readonly #api: Api;
  readonly #session: Session;
  /** Id of the run whose news count (`null`: none running). */
  #run: string | null = null;
  /** Pieces sent and not answered yet, by id, at their offsets in `#known`. */
  #pieces = new Map<number, Piece>();
  #parts = 0;
  #answered = 0;
  /** The text the offsets refer to. */
  #known = "";
  /** Document of the review. */
  #path: string | null = null;
  #nextKey = 0;
  #asked = 0;
  /** What the last run covered (Try again). */
  #scope: EditScope = "paragraph";
  /** Keys of suggestions whose accept waits for the composition to end. */
  #waiting: number[] = [];
  #retry: ReturnType<typeof setTimeout> | null = null;
  #tries = 0;

  constructor(api: Api = defaultApi, session: Session = editing) {
    this.#api = api;
    this.#session = session;
  }

  get review(): ({ kind: "review" } & Review) | null {
    return this.state.kind === "review" ? this.state : null;
  }

  /** A run is going on. */
  get running(): boolean {
    const review = this.review;
    return review !== null && running(review.status);
  }

  /** The bar is open (a review, or "nothing to check"). */
  get open(): boolean {
    return this.state.kind !== "closed";
  }

  // ---- availability + news ----------------------------------------------------------------------------------

  /** Follows availability, run news and the open document until the returned teardown runs. */
  start(): () => void {
    // a window that cannot listen (no backend) stays dormant
    const listening: Promise<Unlisten | null>[] = [
      this.#api.onAppPlugin("local-ai:state", () => void this.ask()),
      this.#api.onAppPlugin("correction:run", (news) => this.news(news)),
    ].map((listen) => listen.catch(() => null));
    void this.ask();
    const stopWatch = $effect.root(() => {
      // the document went away | its file changed on disk under the edits | back to View
      $effect(() => {
        const path = this.#session.path;
        const mode = this.#session.mode;
        const conflict = this.#session.state.s === "conflict";
        untrack(() => this.left(path, mode, conflict));
      });
      // the editor's text changed (after the change check): an external source (the editor), not state mirrored
      // here; `sync` reads it untracked
      $effect(() => {
        void this.#session.text;
        untrack(() => this.sync());
      });
    });
    return () => {
      stopWatch();
      for (const off of listening) void off.then((unlisten) => unlisten?.());
      this.#close();
      this.available = false;
    };
  }

  /** Asks the backend whether a model is assigned. */
  async ask(): Promise<void> {
    const seq = ++this.#asked;
    const answer = await this.#api.pluginCall("correction.status", {});
    if (seq !== this.#asked) return;
    this.available = answer.ok && answer.value.available;
  }

  /** One run's news; other runs' (other windows, old runs) are ignored. */
  news(news: CorrectionRun): void {
    if (this.#run === null || news.run !== this.#run) return;
    const phase = news.phase;
    switch (phase.kind) {
      case "loading":
        this.#status({ kind: "loading", progress: phase.progress });
        return;
      case "piece":
        this.#answer(phase.id, phase.result);
        return;
      case "done":
        this.#end({ kind: "finished" });
        return;
      case "cancelled":
        this.#end({ kind: "cancelled" });
        return;
      case "failed":
        this.#end({
          kind: "failed",
          error: { kind: "backend", command: "plugin_call", error: phase.error },
        });
        return;
      default:
        return phase satisfies never;
    }
  }

  // ---- runs --------------------------------------------------------------------------------------------------

  /** Mod+Alt+K: in review with suggestions -> the keyboard to Accept; else corrects the selection | paragraph. */
  correctHere(): Promise<void> {
    const review = this.review;
    if (review !== null && review.suggestions.length > 0) {
      this.focusRequest += 1;
      this.#reveal();
      return Promise.resolve();
    }
    return this.correct(this.#session.scope("selection") === null ? "paragraph" : "selection");
  }

  /** Starts a run over `kind` of the editor's text (an earlier run of this window is stopped). */
  async correct(kind: EditScope): Promise<void> {
    if (!this.available) return;
    this.#scope = kind;
    this.#stopRun();
    this.#clearWaiting();
    const session = this.#session;
    const text = session.current();
    const range = session.scope(kind);
    const found = range === null ? [] : pieces(text, range);
    this.#known = text;
    this.#path = session.path;
    this.#pieces = new Map(found.map((piece, id) => [id, piece]));
    this.#parts = found.length;
    this.#answered = 0;
    if (found.length === 0) {
      this.state = { kind: "nothing" };
      return;
    }
    const run = newRunId();
    this.#run = run;
    this.state = {
      kind: "review",
      status: { kind: "loading", progress: 0 },
      suggestions: [],
      at: 0,
      discarded: 0,
      outdated: 0,
    };
    const sent = await this.#api.pluginCall("correction.run", {
      run,
      pieces: this.#order(found, session.scope("paragraph")).map((id) => ({
        id,
        text: found[id]?.text ?? "",
      })),
    });
    if (!sent.ok && this.#run === run) this.#end({ kind: "failed", error: sent.error });
  }

  /** Try again after a failure: the same kind of scope, where the caret is now. */
  retry(): Promise<void> {
    return this.correct(this.#scope);
  }

  /** Stops the run; the suggestions found so far stay. */
  cancel(): void {
    const run = this.#run;
    if (run === null) return;
    void this.#api.pluginCall("correction.cancel", { run });
    this.#end({ kind: "cancelled" });
  }

  /** Done: the review closes (a run still going is stopped), the keyboard back to the editor. */
  done(): void {
    this.#close();
    this.#session.focus();
  }

  /** The document of the window changed: a review of another file, a conflict or View closes. */
  left(path: string | null, mode: EditMode, conflict: boolean): void {
    if (this.state.kind === "closed") return;
    if (path !== this.#path || mode === "view" || conflict) this.#close();
  }

  // ---- review ------------------------------------------------------------------------------------------------

  /** Moves to the next (`1`) | previous (`-1`) suggestion (wraps) and shows it. */
  go(step: 1 | -1): void {
    this.sync();
    const review = this.review;
    if (review === null || review.suggestions.length === 0) return;
    const n = review.suggestions.length;
    this.state = { ...review, at: (review.at + step + n) % n };
    this.#reveal();
  }

  accept(): void {
    this.sync();
    const review = this.review;
    const current = review?.suggestions[review.at];
    if (current) this.#put([current.key]);
  }

  reject(): void {
    this.sync();
    const review = this.review;
    const current = review?.suggestions[review.at];
    if (review === null || !current) return;
    this.#settle(
      review.suggestions.filter((s) => s !== current),
      0,
    );
  }

  /** Every suggestion left in one edit (one undo step). Not while the run goes on. */
  acceptAll(): void {
    this.sync();
    const review = this.review;
    if (review === null || running(review.status)) return;
    this.#put(review.suggestions.map((s) => s.key));
  }

  rejectAll(): void {
    const review = this.review;
    if (review === null || running(review.status)) return;
    this.done();
  }

  /** Follows the editor's text: a change not made here drops what it touched and shifts what follows. */
  sync(): void {
    if (this.state.kind === "closed") return;
    const now = this.#session.current();
    if (now === this.#known) return;
    const change = changedRange(this.#known, now);
    this.#known = now;
    const touches = (r: TextRange): boolean => r.from <= change.to && r.to >= change.from;
    const shift = <R extends TextRange>(r: R): R =>
      r.from >= change.to ? { ...r, from: r.from + change.delta, to: r.to + change.delta } : r;
    for (const [id, piece] of this.#pieces) {
      if (touches(piece)) this.#pieces.delete(id);
      else this.#pieces.set(id, shift(piece));
    }
    const review = this.review;
    if (review === null) return;
    const kept = review.suggestions.filter((s) => !touches(s)).map(shift);
    this.#waiting = this.#waiting.filter((key) => kept.some((s) => s.key === key));
    this.#settle(kept, review.suggestions.length - kept.length);
  }

  // ---- inside ------------------------------------------------------------------------------------------------

  /** Piece ids in sending order: those in the caret's block first, then document order. */
  #order(found: readonly Piece[], here: TextRange | null): number[] {
    const ids = found.map((_, id) => id);
    if (here === null) return ids;
    const inside = (p: Piece): boolean => p.from < here.to && p.to > here.from;
    return [
      ...ids.filter((id) => found[id] !== undefined && inside(found[id])),
      ...ids.filter((id) => found[id] !== undefined && !inside(found[id])),
    ];
  }

  #status(status: RunStatus): void {
    const review = this.review;
    if (review !== null) this.state = { ...review, status };
  }

  #answer(id: number, result: CorrectionPieceResult): void {
    this.#answered += 1;
    this.#status({
      kind: "checking",
      part: Math.min(this.#answered + 1, this.#parts),
      parts: this.#parts,
    });
    this.sync();
    const piece = this.#pieces.get(id);
    this.#pieces.delete(id);
    const review = this.review;
    if (piece === undefined || review === null) return;
    if (result.kind === "discarded") {
      this.state = { ...review, discarded: review.discarded + 1 };
      return;
    }
    // the person changed the piece meanwhile: its answer is about other text
    if (this.#known.slice(piece.from, piece.to) !== piece.original) return;
    const verdict = judge(this.#known, piece, result.text);
    if (verdict.kind === "discard") {
      this.state = { ...review, discarded: review.discarded + 1 };
      return;
    }
    if (verdict.suggestions.length === 0) return;
    const added = verdict.suggestions.map((s): Found => ({ ...s, key: this.#nextKey++ }));
    const merged = [...review.suggestions, ...added].sort((a, b) => a.from - b.from);
    const current = review.suggestions[review.at];
    this.state = {
      ...review,
      suggestions: merged,
      at: current === undefined ? 0 : merged.indexOf(current),
    };
  }

  /** The run's end (`status` kept with what was found). */
  #end(status: RunStatus): void {
    this.#run = null;
    this.#pieces.clear();
    this.#status(status);
  }

  /** Puts the suggestions of `keys` into the text in one edit. */
  #put(keys: readonly number[]): void {
    const review = this.review;
    if (review === null) return;
    const chosen = review.suggestions.filter((s) => keys.includes(s.key));
    if (chosen.length === 0) return;
    const text = this.#known;
    const fits = chosen.filter((s) => text.slice(s.from, s.to) === s.original);
    const edits = fits.map(({ from, to, insert, original }): TextEdit => ({
      from,
      to,
      insert,
      expect: original,
    }));
    const stale = chosen.filter((s) => !fits.includes(s));
    if (edits.length > 0 && this.#session.apply(edits)) {
      this.#applied(fits, stale);
      return;
    }
    if (edits.length > 0 && this.#session.composing()) {
      this.#wait(keys);
      return;
    }
    // a batch refused: one edit the editor does not take this way must not cost the others
    if (fits.length > 1) {
      if (stale.length > 0)
        this.#settle(
          review.suggestions.filter((s) => !stale.includes(s)),
          stale.length,
        );
      for (const s of fits)
        if (this.review?.suggestions.some((f) => f.key === s.key)) this.#put([s.key]);
      return;
    }
    // the editor refused (the text moved on, a block it does not edit this way): never applied blind
    this.#settle(
      review.suggestions.filter((s) => !chosen.includes(s)),
      chosen.length,
    );
  }

  /** `done` went in: they leave the list, what follows them moves by the length change; `stale` (no longer read as
   *  found) leave it too, counted. */
  #applied(done: readonly Found[], stale: readonly Found[]): void {
    const review = this.review;
    if (review === null) return;
    const sorted = [...done].sort((a, b) => a.from - b.from);
    const shiftAt = (pos: number): number =>
      sorted.reduce((sum, s) => (s.to <= pos ? sum + s.insert.length - (s.to - s.from) : sum), 0);
    const move = <R extends TextRange>(r: R): R => {
      const delta = shiftAt(r.from);
      return delta === 0 ? r : { ...r, from: r.from + delta, to: r.to + delta };
    };
    let text = this.#known;
    for (const s of [...sorted].reverse())
      text = text.slice(0, s.from) + s.insert + text.slice(s.to);
    this.#known = text;
    for (const [id, piece] of this.#pieces) this.#pieces.set(id, move(piece));
    const gone = new Set([...done, ...stale].map((s) => s.key));
    this.#settle(review.suggestions.filter((s) => !gone.has(s.key)).map(move), stale.length);
    // the editor may write the block its own way (Edit mode): what differs is a change like any other
    this.sync();
  }

  /** The list is now `left` (`dropped` more no longer apply); empty after a finished run -> the review closes. */
  #settle(left: readonly Found[], dropped: number): void {
    const review = this.review;
    if (review === null) return;
    const current = review.suggestions[review.at];
    const kept = current === undefined ? -1 : left.findIndex((s) => s.key === current.key);
    // the current one went: the one after it (now at its index) is current
    const at = kept >= 0 ? kept : Math.min(review.at, Math.max(0, left.length - 1));
    const removed = left.length < review.suggestions.length;
    if (left.length === 0 && removed && !running(review.status) && dropped === 0) {
      this.done();
      return;
    }
    // excerpts read the text as it is now (an accepted neighbor shows corrected)
    const text = this.#known;
    const suggestions = left.map((s) => ({ ...s, ...around(text, s.from, s.to) }));
    this.state = { ...review, suggestions, at, outdated: review.outdated + dropped };
    if (removed && left.length > 0) this.#reveal();
  }

  #reveal(): void {
    const review = this.review;
    const current = review?.suggestions[review.at];
    if (current) this.#session.reveal({ from: current.from, to: current.to });
  }

  /** The editor is composing text: the accept of `keys` is tried again shortly. */
  #wait(keys: readonly number[]): void {
    this.#waiting = [...new Set([...this.#waiting, ...keys])];
    if (this.#retry !== null) return;
    this.#tries = 0;
    const again = (): void => {
      this.#retry = null;
      this.#tries += 1;
      if (this.#session.composing() && this.#tries < COMPOSE_TRIES) {
        this.#retry = setTimeout(again, COMPOSE_RETRY_MS);
        return;
      }
      const waiting = this.#waiting;
      this.#waiting = [];
      this.sync();
      if (!this.#session.composing()) this.#put(waiting);
    };
    this.#retry = setTimeout(again, COMPOSE_RETRY_MS);
  }

  #clearWaiting(): void {
    if (this.#retry !== null) clearTimeout(this.#retry);
    this.#retry = null;
    this.#waiting = [];
  }

  #stopRun(): void {
    const run = this.#run;
    if (run === null) return;
    this.#run = null;
    void this.#api.pluginCall("correction.cancel", { run });
  }

  #close(): void {
    this.#stopRun();
    this.#clearWaiting();
    this.#pieces.clear();
    this.state = CLOSED;
  }
}

/** The review of this window. */
export const correction = new CorrectionStore();
