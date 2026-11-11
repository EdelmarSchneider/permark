// Review of one window over what a model proposes for the open text, piece by piece; a plugin configures it (whose
// backend runs the pieces, which pieces to send, what an answer is worth). A run: the pieces of the editor's text in
// the scope (the plugin's `prepare`, offsets = the editor's text, LF) go to `<plugin>.run`, except those the editor on
// screen cannot change (`session.changeable`; Edit mode: a block kept as written), only counted: their answer could
// not go in. News `<plugin>:run` of this run only (`PieceRunLink`: the run's id, its news, its stop; a run that is
// not reviewed uses it alone); each answer -> the plugin's `judge` -> suggestions (document
// offsets) merged in document order; discarded answers and answers that say the same as their piece only counted,
// each on its own. One review at a time in an editor (the marks of every plugin's review go to the editor's one
// layer; `openReview`: whose it is): while one is open, another plugin's does not run and says `blocked`. Offsets
// follow the text: own edits shift what follows exactly; any other change (typing, the other editor, the editor
// writing a block its own way) -> prefix/suffix diff, the rest shifted, suggestions | pieces touching it dropped
// (counted) unless their text is still told apart inside what changed (`follower`).
// Accept = one checked `session.apply` (expect = original): refused while composing -> retried shortly; refused else
// -> dropped (counted). Accept all = one apply (one undo step); a batch refused (Edit mode: an edit that would make
// its block read as something else, …) -> the edits the editor says it takes (`session.changeable` with the text to
// put in), again in one apply, only the others dropped. File switch | conflict | View -> review closed. Every pending
// suggestion is marked in the text (`session.marks`, each time the list | current one changes; closed -> none); a click
// on a mark makes it current, the caret left where the person put it (typing safety).

import { untrack } from "svelte";
import {
  api as defaultApi,
  type Api,
  type ApiError,
  type AppPluginEventName,
  type AppPluginEvents,
  type Unlisten,
} from "../../../api";
import type { PieceRunPiece } from "../../../api/generated/PieceRunPiece";
import type { PieceRunResult } from "../../../api/generated/PieceRunResult";
import type { PieceRun } from "../../../api/generated/PieceRun";
import {
  editing,
  type EditMode,
  type EditScope,
  type EditingSession,
  type TextEdit,
  type TextRange,
} from "../../../core/editing/session.svelte";
import type { Piece } from "./segment";
import { around, type Suggestion } from "./suggestion";

/** What the review needs of the editing session. */
export type Session = Pick<
  EditingSession,
  | "path"
  | "mode"
  | "state"
  | "text"
  | "current"
  | "scope"
  | "apply"
  | "changeable"
  | "reveal"
  | "marks"
  | "markAt"
  | "composing"
  | "focus"
>;

/** Plugins whose backend runs pieces: news `<plugin>:run`, calls `<plugin>.status | run | cancel`. */
export type PiecePlugin = {
  [E in AppPluginEventName]: AppPluginEvents[E] extends PieceRun
    ? E extends `${infer P}:run`
      ? P
      : never
    : never;
}[AppPluginEventName];

/** What a plugin makes of one answer: its suggestions in document offsets, the piece left as it was (an answer not
 *  to trust), or nothing to suggest because the answer says the same as the piece. */
export type PieceVerdict =
  | { kind: "ok"; suggestions: Suggestion[] }
  | { kind: "discard"; reason: string }
  | { kind: "same" };

/** What makes the review one plugin's feature. */
export type ReviewConfig = {
  /** The plugin whose backend runs the pieces (its calls + news carry this id). */
  plugin: PiecePlugin;
  /** Pieces of `text` inside `range` to send, in document order. */
  prepare: (text: string, range: TextRange) => Piece[];
  /** Verdict on the model's `output` for `piece` (cut from `doc`). */
  judge: (doc: string, piece: Piece, output: string) => PieceVerdict;
  /** What `<plugin>.run` takes beside the run id + the pieces (read when a run starts). */
  runArgs: () => Readonly<Record<string, unknown>>;
};

/** A suggestion with its identity (offsets change, the key stays). */
export type Found = Suggestion & { key: number };

export type RunStatus =
  | { kind: "engine"; receivedBytes: number; totalBytes: number }
  | { kind: "loading"; progress: number }
  | { kind: "checking"; part: number; parts: number }
  | { kind: "finished" }
  | { kind: "cancelled" }
  | { kind: "failed"; error: ApiError };

/** Where a run stands before its first answer: the engine being fetched | the model getting ready. */
export type RunPreparing = Extract<RunStatus, { kind: "engine" | "loading" }>;
/** How a run ended. */
export type RunEnd = Extract<RunStatus, { kind: "finished" | "cancelled" | "failed" }>;

/** What a run tells whoever started it. */
export type RunHearer = {
  preparing(status: RunPreparing): void;
  /** The model's answer for the piece sent as `id`. */
  answer(id: number, result: PieceRunResult): void;
  /** The run is over: no news of it follow. */
  end(status: RunEnd): void;
};

export type Review = {
  status: RunStatus;
  /** In document order. */
  suggestions: readonly Found[];
  /** Index of the current suggestion (0 when none). */
  at: number;
  /** Suggestions the run found in all: those left in `suggestions` and those handled since. */
  total: number;
  /** Pieces left as they were (the model changed too much | the piece was too long). */
  discarded: number;
  /** Pieces whose answer said the same as the piece (verdict `same`). */
  same: number;
  /** Suggestions dropped because the text changed under them. */
  outdated: number;
  /** Pieces of the scope not sent: the editor on screen cannot change their text. */
  locked: number;
};

/** `nothing`: the scope held no piece to send (`locked`: pieces left out because the editor on screen cannot change
 *  their text). */
export type ReviewState =
  { kind: "closed" } | { kind: "nothing"; locked: number } | ({ kind: "review" } & Review);

/** Wait between tries of an edit refused while text is composed (ms), and how many tries. */
const COMPOSE_RETRY_MS = 100;
const COMPOSE_TRIES = 50;

const CLOSED: ReviewState = { kind: "closed" };

export function running(status: RunStatus): boolean {
  return status.kind === "engine" || status.kind === "loading" || status.kind === "checking";
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

/** Chars of a changed part past which nothing is looked for inside it. */
const FOLLOW_CHARS = 20_000;
/** A run of letters | digits. */
const WORD = /[\p{L}\p{N}]+/gu;

/** Starts of each `needle` inside `text[from, to)`. */
function places(text: string, needle: string, from: number, to: number): number[] {
  const found: number[] = [];
  let at = text.indexOf(needle, from);
  while (at >= 0 && at + needle.length <= to) {
    found.push(at);
    at = text.indexOf(needle, at + 1);
  }
  return found;
}

/** The word before and the word after `from..to` in `text`, as far as its excerpt reaches (none: empty). */
function wordsAround(text: string, from: number, to: number): string {
  const { before, after } = around(text, from, to);
  return `${before.match(WORD)?.at(-1) ?? ""} ${after.match(WORD)?.[0] ?? ""}`;
}

/**
 * Where the ranges of `was` are in `now`, by the one part that differs between them: a range before it stays, one
 * after it moves by the length change, one it touches is gone (`null`) unless its text is still told apart inside the
 * part: there as many times as before (the same one of them, in order), between the same two words. So a change that
 * only moves what is around a text (a table's columns padded anew, words typed elsewhere in a block the editor then
 * writes again) keeps its range; a change of its own words, or of the word next to them, drops it. An empty range
 * has no text to find: touched = gone.
 */
function follower(
  was: string,
  now: string,
): <R extends TextRange & { original: string }>(range: R) => R | null {
  const change = changedRange(was, now);
  const end = change.to + change.delta;
  const searched = Math.max(change.to, end) - change.from <= FOLLOW_CHARS;
  return (range) => {
    if (range.to < change.from) return range;
    let by = change.delta;
    if (range.from <= change.to) {
      const text = range.original;
      if (!searched || text === "" || range.from < change.from || range.to > change.to) return null;
      const before = places(was, text, change.from, change.to);
      const after = places(now, text, change.from, end);
      const at = after[before.indexOf(range.from)];
      if (at === undefined || before.length !== after.length) return null;
      if (wordsAround(was, range.from, range.to) !== wordsAround(now, at, at + text.length))
        return null;
      by = at - range.from;
    }
    return by === 0 ? range : { ...range, from: range.from + by, to: range.to + by };
  };
}

function newRunId(): string {
  return globalThis.crypto.randomUUID();
}

/**
 * A window's line to one run at a time in a plugin's backend: starts it under an id of its own, hears the news of that
 * run alone (`<plugin>:run` carries every window's runs, and old ones), stops it. Whoever runs pieces without a
 * review of them (a whole document translated into a new file) uses it as the review does.
 */
export class PieceRunLink {
  readonly #plugin: PiecePlugin;
  readonly #api: Api;
  readonly #hearer: RunHearer;
  /** Id of the run whose news count (`null`: none running). */
  #run: string | null = null;

  constructor(plugin: PiecePlugin, api: Api, hearer: RunHearer) {
    this.#plugin = plugin;
    this.#api = api;
    this.#hearer = hearer;
  }

  /** Hears the plugin's run news until the returned removal runs; a window that cannot listen (no backend) hears
   *  none. */
  listen(): () => void {
    const listening: Promise<Unlisten | null> = this.#api
      .onAppPlugin(`${this.#plugin}:run`, (news) => this.news(news))
      .catch(() => null);
    return () => void listening.then((unlisten) => unlisten?.());
  }

  /** Starts a run over `pieces` (one still going is stopped first); `args`: what the plugin's run takes beside its
   *  id + the pieces. A run the backend refuses ends at once (`failed`). */
  async start(pieces: PieceRunPiece[], args: Readonly<Record<string, unknown>>): Promise<void> {
    this.stop();
    const run = newRunId();
    this.#run = run;
    const sent = await this.#api.pluginCall(`${this.#plugin}.run`, { ...args, run, pieces });
    if (!sent.ok && this.#run === run) this.#end({ kind: "failed", error: sent.error });
  }

  /** One run's news; other runs' (other windows, old runs) are ignored. */
  news(news: PieceRun): void {
    if (this.#run === null || news.run !== this.#run) return;
    const phase = news.phase;
    switch (phase.kind) {
      case "engine":
        this.#hearer.preparing({
          kind: "engine",
          receivedBytes: phase.receivedBytes,
          totalBytes: phase.totalBytes,
        });
        return;
      case "loading":
        this.#hearer.preparing({ kind: "loading", progress: phase.progress });
        return;
      case "piece":
        this.#hearer.answer(phase.id, phase.result);
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

  /** Stops the run going on: its news count no more and its end is not told (the caller knows). `false`: none was
   *  going. */
  stop(): boolean {
    const run = this.#run;
    if (run === null) return false;
    this.#run = null;
    void this.#api.pluginCall(`${this.#plugin}.cancel`, { run });
    return true;
  }

  #end(status: RunEnd): void {
    this.#run = null;
    this.#hearer.end(status);
  }
}

/** Whose review is open in an editor (`null`: none). */
class Turn {
  holder = $state.raw<PieceReview | null>(null);
}

/** The turn of each editing session: every review of a window shares the session, and its editor's one mark layer. */
const turns = new WeakMap<Session, Turn>();

function turnOf(session: Session): Turn {
  let turn = turns.get(session);
  if (turn === undefined) {
    turn = new Turn();
    turns.set(session, turn);
  }
  return turn;
}

/** The review open in the editor of `session` (this window's, by default); `null`: none holds it. */
export function openReview(session: Session = editing): PieceReview | null {
  return turnOf(session).holder;
}

/** A suggestion as the checked edit that puts it in. */
function editOf({ from, to, insert, original }: Found): TextEdit {
  return { from, to, insert, expect: original };
}

export class PieceReview {
  #state = $state.raw<ReviewState>(CLOSED);
  /** A model is assigned to the plugin's job (dormant until then). */
  available = $state(false);
  /** Bumped when the keyboard should go to the bar's first action (Accept). */
  focusRequest = $state(0);

  readonly #config: ReviewConfig;
  readonly #api: Api;
  readonly #session: Session;
  /** Whose review is open in the session's editor. */
  readonly #turn: Turn;
  /** The run in the plugin's backend. */
  readonly #runs: PieceRunLink;
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
  /** The list + current index last marked in the text (same list, same index -> not marked again). */
  #marked: { list: readonly Found[]; at: number | null } = { list: [], at: null };

  constructor(config: ReviewConfig, api: Api = defaultApi, session: Session = editing) {
    this.#config = config;
    this.#api = api;
    this.#session = session;
    this.#turn = turnOf(session);
    this.#runs = new PieceRunLink(config.plugin, api, {
      preparing: (status) => this.#status(status),
      answer: (id, result) => this.#answer(id, result),
      end: (status) => this.#end(status),
    });
  }

  /** The plugin this review is set up for. */
  get plugin(): PiecePlugin {
    return this.#config.plugin;
  }

  get state(): ReviewState {
    return this.#state;
  }

  /** Every change of the state marks the text anew; a review (a run going on | suggestions to go through) holds the
   *  editor until it is no longer one. */
  set state(next: ReviewState) {
    this.#state = next;
    if (next.kind === "review") this.#turn.holder = this;
    else if (this.#turn.holder === this) this.#turn.holder = null;
    this.#mark();
  }

  /** Another plugin's review is open in this editor: no run starts here until it closes. */
  get blocked(): boolean {
    const holder = this.#turn.holder;
    return holder !== null && holder !== this;
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
    const listening: Promise<Unlisten | null> = this.#api
      .onAppPlugin("local-ai:state", () => void this.ask())
      .catch(() => null);
    const stopNews = this.#runs.listen();
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
      void listening.then((unlisten) => unlisten?.());
      stopNews();
      this.#close();
      this.available = false;
    };
  }

  /** Asks the backend whether a model is assigned. */
  async ask(): Promise<void> {
    const seq = ++this.#asked;
    const answer = await this.#api.pluginCall(`${this.#config.plugin}.status`, {});
    if (seq !== this.#asked) return;
    this.available = answer.ok && answer.value.available;
  }

  /** News of the plugin's runs: only this review's own run counts. */
  news(news: PieceRun): void {
    this.#runs.news(news);
  }

  // ---- runs --------------------------------------------------------------------------------------------------

  /** The plugin's key: in review with suggestions -> the one marked at the caret current
   *  (if any), the keyboard to Accept; else a run over the selection | paragraph. */
  runHere(): Promise<void> {
    const review = this.review;
    if (review !== null && review.suggestions.length > 0) {
      const at = this.#session.markAt();
      const marked = at === null ? undefined : this.#marked.list[at];
      if (marked) this.pick(marked.key);
      this.focusRequest += 1;
      this.#reveal();
      return Promise.resolve();
    }
    return this.runOver(this.#session.scope("selection") === null ? "paragraph" : "selection");
  }

  /** Starts a run over `kind` of the editor's text (an earlier run of this review is stopped). Pieces the editor on
   *  screen cannot change are left out and counted. Nothing while another plugin's review is open (`blocked`). */
  async runOver(kind: EditScope): Promise<void> {
    if (!this.available || this.blocked) return;
    this.#scope = kind;
    this.#runs.stop();
    this.#clearWaiting();
    const session = this.#session;
    const text = session.current();
    const range = session.scope(kind);
    const cut = range === null ? [] : this.#config.prepare(text, range);
    const found = cut.filter((piece) => session.changeable(piece));
    const locked = cut.length - found.length;
    this.#known = text;
    this.#path = session.path;
    this.#pieces = new Map(found.map((piece, id) => [id, piece]));
    this.#parts = found.length;
    this.#answered = 0;
    if (found.length === 0) {
      this.state = { kind: "nothing", locked };
      return;
    }
    this.state = {
      kind: "review",
      status: { kind: "loading", progress: 0 },
      suggestions: [],
      at: 0,
      total: 0,
      discarded: 0,
      same: 0,
      outdated: 0,
      locked,
    };
    await this.#runs.start(
      this.#order(found, session.scope("paragraph")).map((id) => ({
        id,
        text: found[id]?.text ?? "",
      })),
      this.#config.runArgs(),
    );
  }

  /** Try again after a failure: the same kind of scope, where the caret is now. */
  retry(): Promise<void> {
    return this.runOver(this.#scope);
  }

  /** Stops the run; the suggestions found so far stay. */
  cancel(): void {
    if (this.#runs.stop()) this.#end({ kind: "cancelled" });
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

  /** The suggestion of `key` becomes current (a click on its mark); the caret stays where it is. */
  pick(key: number): void {
    this.sync();
    const review = this.review;
    const at = review?.suggestions.findIndex((s) => s.key === key) ?? -1;
    if (review === null || at < 0 || at === review.at) return;
    this.state = { ...review, at };
  }

  /** The current suggestion selected in the text (the bar's excerpt), the keyboard to the editor. */
  show(): void {
    this.sync();
    if (this.review?.suggestions[this.review.at] === undefined) return;
    this.#reveal();
    this.#session.focus();
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

  /** Follows the editor's text: a change not made here shifts what follows it and drops what it touched, unless that
   *  is still found inside it (`follower`). */
  sync(): void {
    if (this.state.kind === "closed") return;
    const was = this.#known;
    const now = this.#session.current();
    if (now === was) return;
    this.#known = now;
    const follow = follower(was, now);
    for (const [id, piece] of this.#pieces) {
      const moved = follow(piece);
      if (moved === null) this.#pieces.delete(id);
      else this.#pieces.set(id, moved);
    }
    const review = this.review;
    if (review === null) return;
    const kept = review.suggestions.flatMap((s) => follow(s) ?? []);
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

  #answer(id: number, result: PieceRunResult): void {
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
    const verdict = this.#config.judge(this.#known, piece, result.text);
    if (verdict.kind === "discard") {
      this.state = { ...review, discarded: review.discarded + 1 };
      return;
    }
    if (verdict.kind === "same") {
      this.state = { ...review, same: review.same + 1 };
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
      total: review.total + added.length,
    };
  }

  /** The run's end (`status` kept with what was found). */
  #end(status: RunEnd): void {
    this.#pieces.clear();
    this.#status(status);
  }

  /** Puts the suggestions of `keys` into the text in one edit: one undo step, also when the editor takes only some
   *  of them. */
  #put(keys: readonly number[]): void {
    const review = this.review;
    if (review === null) return;
    const chosen = review.suggestions.filter((s) => keys.includes(s.key));
    if (chosen.length === 0) return;
    const text = this.#known;
    const fits = chosen.filter((s) => text.slice(s.from, s.to) === s.original);
    /** `done` go in as one edit; the rest of `chosen` leaves the list, counted. */
    const put = (done: readonly Found[]): boolean => {
      if (done.length === 0 || !this.#session.apply(done.map(editOf))) return false;
      this.#applied(
        done,
        chosen.filter((s) => !done.includes(s)),
      );
      return true;
    };
    if (put(fits)) return;
    if (fits.length > 0 && this.#session.composing()) {
      this.#wait(keys);
      return;
    }
    // a batch refused: one edit the editor does not take must not cost the others, and they still go in together
    const taken = fits.filter((s) => this.#session.changeable(s, s.insert));
    if (taken.length < fits.length && put(taken)) return;
    // the editor refused (the text moved on, a block it does not edit this way): never applied blind
    this.#settle(
      review.suggestions.filter((s) => !chosen.includes(s)),
      chosen.length,
    );
  }

  /** `done` went in: they leave the list, what follows them moves by the length change; `stale` (no longer read as
   *  found | not taken by the editor) leave it too, counted. */
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
    // excerpts read the text as it is now (an accepted neighbor shows as changed)
    const text = this.#known;
    const suggestions = left.map((s) => ({ ...s, ...around(text, s.from, s.to) }));
    this.state = { ...review, suggestions, at, outdated: review.outdated + dropped };
    if (removed && left.length > 0) this.#reveal();
  }

  /** Marks the pending suggestions in the text (none outside a review). */
  #mark(): void {
    const review = this.review;
    const list = review?.suggestions ?? [];
    const at = review === null || list.length === 0 ? null : review.at;
    const last = this.#marked;
    if ((list === last.list || (list.length === 0 && last.list.length === 0)) && at === last.at)
      return;
    this.#marked = { list, at };
    const keys = list.map((s) => s.key);
    this.#session.marks(
      list.map(({ from, to }) => ({ from, to })),
      at,
      (index) => {
        const key = keys[index];
        if (key !== undefined) this.pick(key);
      },
    );
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

  #close(): void {
    this.#runs.stop();
    this.#clearWaiting();
    this.#pieces.clear();
    this.state = CLOSED;
  }
}
