// The Explorer of one window: the list the backend keeps for it, what is open, selected, being renamed, dragged, and
// every action on files. The backend owns the truth (tree, files, undo); this mirrors it through snapshots + deltas
// and sends actions. UI modes are tagged unions; actions are methods called from event handlers.

import { confirmLeave } from "../editing/guard";
import { editing } from "../editing/session.svelte";
import { DEFAULT_REFERENCE_TEMPLATE, formatReference } from "../doc/reference";
import {
  type Api,
  type ApiError,
  type LinkUpdate,
  type MoveOutcome,
  type MovePlan,
  type MoveSpec,
  type NewKind,
  type OpenMode,
  type RootNode,
  type TreeChanged,
  type TreeFilter,
  type TreeNode,
  type Unlisten,
} from "../../api";
import { IDENTITY_UNKNOWN } from "../../api/error";
import { t, tn } from "../i18n";
import type { NoticeAction } from "../window.svelte";
import { CODE_OFFER_KEY, isAnswer, shouldOfferCode, withCode, type Answer } from "./codeOffer";
import { DEFAULT_FILTER, filterToJson, normalizeFilter } from "./filter";
import {
  ancestorsOf,
  applyDelta,
  canDrop,
  depthOf,
  movesInto,
  nameOf,
  parentOf,
  rangeBetween,
  remapSet,
  rootOf,
  rowKey,
  rowsOf,
  topmost,
  type Row,
} from "./list";

const SESSION_FILTER = "explorer.filter";
/** An outside drag resting this long on a closed folder opens it. */
export const AUTO_EXPAND_MS = 700;
const SESSION_EXPANDED = "explorer.expanded";
/** Folders remembered open (a project with thousands of open folders is not a state worth keeping). */
const MAX_REMEMBERED = 500;

/** The backend has no tree for this view right now: no project on it (another folder opening in it) | its tree not open
 *  yet | gone (`unsupported` | `unavailable`). Keeping the open document listed is background work: nothing to tell. */
export function treeGone(error: ApiError): boolean {
  return (
    error.kind === "backend" &&
    (error.error.kind === "unsupported" || error.error.kind === "unavailable")
  );
}

/** The longest a Refresh waits for its list (a backend that never tells one must not leave it working forever). */
export const REFRESH_WAIT_MS = 5000;

/** The person's Refresh: `reading` from the ask until the backend tells this view its list again. */
export type Refreshing = { status: "idle" } | { status: "reading" };

/** The quiet offer to show the code files the filter hides (`codeOffer.ts`). */
export type CodeOffer = { status: "none" } | { status: "offered"; hidden: number };

export type Load =
  { status: "loading" } | { status: "ready" } | { status: "failed"; error: ApiError };

/** What the inline field is for: renaming an entry, or naming a new one inside a folder. */
export type Edit =
  | { mode: "none" }
  | { mode: "rename"; path: string; value: string; problem: EditProblem | null }
  | { mode: "new"; kind: NewKind; dir: string; value: string; problem: EditProblem | null };

/** A move waiting for the person's answer about its links. */
export type Dialog = { status: "closed" } | { status: "asking"; plan: MovePlan };

export type Drag =
  { status: "idle" } | { status: "dragging"; paths: readonly string[]; over: string | null };

/** What the Explorer needs from its window. */
export type ExplorerHost = {
  /** A failed action, worded by the window's notices. */
  fail(error: ApiError): void;
  /** A quiet note ("Moved ...", "Undone"), with buttons when there is something to do about it (Undo). */
  note(text: string, ...actions: NoticeAction[]): void;
  /** The view this Explorer lives in: tree events name their view, and every view hears every event. */
  view(): string;
  linkUpdate(): LinkUpdate;
  /** Opens a folder as a project (the window asks "this window | new window"). */
  openProject(path: string): void;
  /** The reference template (setting); the default when the window gives none. */
  referenceTemplate?(): string;
  /** Asks whether to initialize PerMark in a folder (`path` absolute). */
  initProject?(path: string, name: string): void;
};

/** Reasons a name cannot be used, as the backend words them in `detail`. */
const NAME_PROBLEMS = [
  "empty",
  "chars",
  "edge",
  "dot",
  "reserved",
  "tooLong",
  "notMarkdown",
] as const;
export type NameProblem = (typeof NAME_PROBLEMS)[number];

/** What can be wrong with the name being typed: a rule of names, the name is taken, or the drive cannot tell whether
 *  a name that differs only in case is the same file. */
export type EditProblem = NameProblem | "exists" | "identityUnknown";

export function isNameProblem(text: string | null): text is NameProblem {
  return NAME_PROBLEMS.some((p) => p === text);
}

/** A quick check while typing; the backend has the last word. */
export function quickNameProblem(name: string): NameProblem | null {
  if (name.trim() === "") return "empty";
  if (name === "." || name === "..") return "dot";
  if (/[\\/:*?"<>|]/.test(name) || Array.from(name).some((c) => c < " ")) return "chars";
  if (name.startsWith(" ") || name.endsWith(" ") || name.endsWith(".")) return "edge";
  return null;
}

export class ExplorerStore {
  readonly #api: Api;
  readonly #host: ExplorerHost;

  load = $state.raw<Load>({ status: "loading" });
  roots = $state.raw<readonly RootNode[]>([]);
  nodes = $state.raw<readonly TreeNode[]>([]);
  filter = $state.raw<TreeFilter>(DEFAULT_FILTER);
  invalidPatterns = $state.raw<readonly string[]>([]);
  totalFiles = $state(0);
  shownFiles = $state(0);
  /** Asked once per project whether to show its code files. */
  codeOffer = $state.raw<CodeOffer>({ status: "none" });
  expanded = $state.raw<ReadonlySet<string>>(new Set());
  collapsedRoots = $state.raw<ReadonlySet<string>>(new Set());
  selection = $state.raw<ReadonlySet<string>>(new Set());
  /** The row the keyboard works on. */
  focus = $state<string | null>(null);
  edit = $state.raw<Edit>({ mode: "none" });
  dialog = $state.raw<Dialog>({ status: "closed" });
  drag = $state.raw<Drag>({ status: "idle" });
  /** The folder files dragged in from outside would land in (`""` = the project folder; `null`: no such drag). */
  outside = $state<string | null>(null);
  /** Where an outside drag over the list is (the list scrolls near its edges); `null`: none. */
  outsidePoint = $state.raw<{ x: number; y: number } | null>(null);
  /** The list's geometry: the folder a point on screen belongs to (`null`: outside the list). Set by the panel. */
  geometry: ((x: number, y: number) => string | null) | null = null;
  /** A request to bring a row into view (`n` makes repeated requests for one path distinct). */
  scrollTo = $state.raw<{ path: string; n: number } | null>(null);
  /** Whether a Refresh is under way (its button shows it; asking again meanwhile does nothing). */
  refreshing = $state.raw<Refreshing>({ status: "idle" });

  rows = $derived<Row[]>(rowsOf(this.roots, this.nodes, this.expanded, this.collapsedRoots));

  #seq = 0;
  /** Whole lists asked for (the latest answer wins: an older one landing after it would bring back a stale list). */
  #opens = 0;
  #closed = false;
  #off: Unlisten | null = null;
  #anchor: string | null = null;
  #scrolls = 0;
  /** Deltas that arrived before the first list. */
  #early: TreeChanged[] = [];
  #timer: ReturnType<typeof setTimeout> | null = null;
  #active: string | null = null;
  /** What the person answered to the code offer, for this project (`null`: not yet). */
  #answered: Answer | null = null;
  /** Folders (and worktree roots) an outside drag opened by resting on them: closed again unless a drop lands there. */
  #autoOpened = new Set<string>();
  #autoTimer: ReturnType<typeof setTimeout> | null = null;
  /** The closed folder the auto-open timer waits on. */
  #autoFor: string | null = null;
  /** Paths to select once they reach the list (items an import put in place). */
  #toSelect = new Set<string>();
  /** Ends a Refresh no list answered. */
  #refreshTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(api: Api, host: ExplorerHost) {
    this.#api = api;
    this.#host = host;
  }

  // ---- lifecycle -------------------------------------------------------------------------------------------------

  /** Listens, then loads the list (no change is lost in between). Resolves once the list is loaded, with the
   *  teardown (`close`, which also works before that). */
  async start(): Promise<Unlisten> {
    const off = await this.#api.on("tree-changed", (event) => this.#onChanged(event));
    if (this.#closed) off();
    else {
      this.#off = off;
      await this.#open();
    }
    return () => this.close();
  }

  /** Stops listening and closes the view's list; works at any time, also while the list still loads. The backend
   *  keeps one list per view, whatever project the view shows by then: a closed store asks for nothing more and drops
   *  answers still on their way, so it never closes | resets the list of the project that came after it. A closed
   *  store stays closed (one store per sidebar: the next one is a new store), so every action that would reach the
   *  backend does nothing. */
  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#off?.();
    this.#off = null;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    this.#clearAuto();
    this.#refreshed();
    void this.#api.treeClose();
  }

  /** The list asked for as `ask` is still wanted: the store is open and no newer list was asked for since. */
  #wanted(ask: number): boolean {
    return !this.#closed && ask === this.#opens;
  }

  async #open(): Promise<void> {
    if (this.#closed) return;
    const ask = ++this.#opens;
    this.load = { status: "loading" };
    const remembered = await this.#api.projectSessionGet(SESSION_FILTER);
    if (!this.#wanted(ask)) return;
    const filter = remembered.ok ? normalizeFilter(remembered.value) : null;
    const answer = await this.#api.projectSessionGet(CODE_OFFER_KEY);
    if (!this.#wanted(ask)) return;
    this.#answered = answer.ok && isAnswer(answer.value) ? answer.value : null;
    const open = await this.#api.projectSessionGet(SESSION_EXPANDED);
    if (!this.#wanted(ask)) return;
    if (open.ok && Array.isArray(open.value)) {
      this.expanded = new Set(open.value.filter((v): v is string => typeof v === "string"));
    }
    const result = await this.#api.treeOpen(filter);
    if (!this.#wanted(ask)) return;
    if (!result.ok) {
      this.load = { status: "failed", error: result.error };
      return;
    }
    this.#take(result.value);
    this.load = { status: "ready" };
    // deltas that came in while the list was loading
    const early = this.#early.sort((a, b) => a.delta.seq - b.delta.seq);
    this.#early = [];
    for (const event of early) this.#onChanged(event);
    if (this.#active !== null) await this.#follow(this.#active);
  }

  #take(snapshot: {
    roots: readonly RootNode[];
    nodes: readonly TreeNode[];
    filter: TreeFilter;
    seq: number;
    invalidPatterns: readonly string[];
    totalFiles: number;
    shownFiles: number;
    codeHidden: number;
  }): void {
    this.roots = snapshot.roots;
    this.nodes = snapshot.nodes;
    this.filter = snapshot.filter;
    this.invalidPatterns = snapshot.invalidPatterns;
    this.totalFiles = snapshot.totalFiles;
    this.shownFiles = snapshot.shownFiles;
    this.codeOffer = shouldOfferCode({
      codeHidden: snapshot.codeHidden,
      totalFiles: snapshot.totalFiles,
      filter: snapshot.filter,
      answered: this.#answered,
    })
      ? { status: "offered", hidden: snapshot.codeHidden }
      : { status: "none" };
    this.#seq = snapshot.seq;
    const alive = new Set(snapshot.nodes.map((n) => n.path));
    this.selection = new Set(Array.from(this.selection).filter((p) => alive.has(p)));
    if (this.focus !== null && !alive.has(this.focus)) this.focus = null;
  }

  #onChanged(event: TreeChanged): void {
    // another view's list (another tab's project): its numbering is not ours. Taking it would skip our next delta (a
    // file just copied in stayed out of the list until a reload: director 2026-10-20)
    if (event.window !== this.#host.view()) return;
    // the backend tells this view its list after reading the folder again, also when nothing differs
    this.#refreshed();
    if (this.load.status !== "ready") {
      this.#early.push(event);
      return;
    }
    const delta = event.delta;
    if (delta.seq <= this.#seq) return;
    if (delta.seq !== this.#seq + 1) {
      // a delta was missed: ask for the whole list again
      void this.#open();
      return;
    }
    this.#seq = delta.seq;
    // the project's default moved and this window follows it
    if (delta.filter !== null) this.filter = delta.filter;
    // state that follows renames moves before the list changes
    this.expanded = remapSet(this.expanded, delta.renamed);
    this.selection = remapSet(this.selection, delta.renamed);
    if (this.focus !== null) {
      this.focus =
        remapSet(new Set([this.focus]), delta.renamed)
          .values()
          .next().value ?? null;
    }
    this.nodes = applyDelta(this.nodes, delta);
    this.totalFiles = delta.totalFiles;
    this.shownFiles = delta.shownFiles;
    const alive = new Set(this.nodes.map((n) => n.path));
    if (this.selection.size > 0) {
      this.selection = new Set(Array.from(this.selection).filter((p) => alive.has(p)));
    }
    if (this.focus !== null && !alive.has(this.focus)) this.focus = null;
    this.#selectArrived(alive);
  }

  async retry(): Promise<void> {
    await this.#open();
  }

  /** The person's Refresh: the backend reads the project folder again and tells what differs through deltas (the list
   *  is not asked for again: it keeps its place, what is open and selected). One at a time: asked again while one runs
   *  (a second press, a held key) it does nothing. It runs until this view is told its list (the backend tells it
   *  after the walk, also when nothing differs), a refusal, or `REFRESH_WAIT_MS`. A view with no project now (another
   *  folder opening in it) has nothing to read. */
  async refresh(): Promise<void> {
    if (this.#closed || this.refreshing.status === "reading") return;
    this.refreshing = { status: "reading" };
    this.#refreshTimer = setTimeout(() => this.#refreshed(), REFRESH_WAIT_MS);
    const result = await this.#api.treeRefresh();
    if (result.ok) return;
    this.#refreshed();
    if (!treeGone(result.error)) this.#host.fail(result.error);
  }

  /** The Refresh under way, if any, is over. */
  #refreshed(): void {
    if (this.#refreshTimer !== null) clearTimeout(this.#refreshTimer);
    this.#refreshTimer = null;
    if (this.refreshing.status !== "idle") this.refreshing = { status: "idle" };
  }

  #remember(): void {
    if (this.#closed) return;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.#api.projectSessionSet(SESSION_FILTER, filterToJson(this.filter));
      // folders a passing drag opened are not the person's choice (yet)
      void this.#api.projectSessionSet(
        SESSION_EXPANDED,
        Array.from(this.expanded)
          .filter((p) => !this.#autoOpened.has(p))
          .slice(0, MAX_REMEMBERED),
      );
    }, 400);
  }

  // ---- filter ----------------------------------------------------------------------------------------------------

  async setFilter(filter: TreeFilter): Promise<void> {
    if (this.#closed) return;
    const result = await this.#api.treeSetFilter(filter);
    if (this.#closed) return;
    if (!result.ok) {
      this.#host.fail(result.error);
      return;
    }
    this.#take(result.value);
    this.#remember();
    if (this.#active !== null) await this.#follow(this.#active);
  }

  /** "Show code files": the filter shows them from now on, and the project is not asked again. */
  async acceptCodeOffer(): Promise<void> {
    await this.#answer("shown");
    await this.setFilter(withCode(this.filter));
  }

  /** "No thanks": the project is not asked again. */
  async declineCodeOffer(): Promise<void> {
    await this.#answer("declined");
  }

  async #answer(answer: Answer): Promise<void> {
    if (this.#closed) return;
    this.#answered = answer;
    this.codeOffer = { status: "none" };
    await this.#api.projectSessionSet(CODE_OFFER_KEY, answer);
  }

  /** Back to the project's default filter. */
  async resetFilter(): Promise<void> {
    if (this.#closed) return;
    const result = await this.#api.treeOpen(null);
    if (this.#closed) return;
    if (!result.ok) {
      this.#host.fail(result.error);
      return;
    }
    this.#take(result.value);
    this.#remember();
  }

  // ---- open / close folders --------------------------------------------------------------------------------------

  isOpen(path: string): boolean {
    return this.expanded.has(path);
  }

  async toggle(path: string): Promise<void> {
    if (this.expanded.has(path)) {
      const next = new Set(this.expanded);
      next.delete(path);
      this.expanded = next;
      this.#remember();
      return;
    }
    await this.#openFolders([path]);
  }

  /**
   * The one way a folder opens: mark it expanded AND ask the backend, which is what makes its
   * children enter the list (`unlisted` counts drop, rows arrive). Expanding without asking leaves
   * rows the store cannot select, so every expansion comes through here. The folders above each path
   * go first: a folder the list does not have yet is only reachable once its parent is opened.
   */
  async #openFolders(paths: readonly string[]): Promise<void> {
    if (this.#closed) return;
    const chain = new Set<string>();
    for (const p of paths) {
      if (p !== "") chain.add(p);
      for (const a of ancestorsOf(p)) chain.add(a);
    }
    const closed = Array.from(chain)
      .filter((p) => !this.expanded.has(p))
      .sort((a, b) => depthOf(a) - depthOf(b));
    if (closed.length === 0) return;
    const next = new Set(this.expanded);
    for (const p of closed) next.add(p);
    this.expanded = next;
    this.#remember();
    for (const p of closed) {
      const node = this.nodes.find((n) => n.path === p);
      if (this.#closed) return;
      if (node?.kind.type === "dir" && node.kind.unlisted !== null) {
        const result = await this.#api.treeOpenFolder(p);
        if (!result.ok) this.#host.fail(result.error);
      }
    }
  }

  toggleRoot(path: string): void {
    const next = new Set(this.collapsedRoots);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    this.collapsedRoots = next;
  }

  collapseAll(): void {
    this.expanded = new Set();
    this.collapsedRoots = new Set();
    this.#remember();
  }

  // ---- selection and focus ---------------------------------------------------------------------------------------

  /** `single`: only this; `toggle`: add | remove (Ctrl | Cmd); `range`: from the anchor (Shift). */
  select(path: string, how: "single" | "toggle" | "range"): void {
    switch (how) {
      case "single":
        this.selection = new Set([path]);
        this.#anchor = path;
        break;
      case "toggle": {
        const next = new Set(this.selection);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        this.selection = next;
        this.#anchor = path;
        break;
      }
      case "range":
        this.selection = new Set(rangeBetween(this.rows, this.#anchor ?? path, path));
        break;
      default:
        how satisfies never;
    }
    this.focus = path;
  }

  /** Select all: every row the list shows now (files and folders, inside open folders too), which is what Home, then
   *  Shift+End reaches. The row the keyboard is on stays; with none (or one a closed folder took away) the first row
   *  takes it, and a Shift + arrow | click after it extends from there. */
  selectAll(): void {
    const keys = this.rows.filter((r) => r.type === "node").map(rowKey);
    const first = keys[0];
    const last = keys[keys.length - 1];
    if (first === undefined || last === undefined) return;
    this.selection = new Set(rangeBetween(this.rows, first, last));
    if (this.focus === null || !keys.includes(this.focus)) {
      this.focus = first;
      this.#anchor = first;
    }
  }

  /** Arrow keys: the row `step` away from the focused one (clamped). */
  move(step: number, extend: boolean): void {
    const keys = this.rows.filter((r) => r.type === "node").map(rowKey);
    if (keys.length === 0) return;
    const at = this.focus === null ? -1 : keys.indexOf(this.focus);
    const next = keys[Math.min(keys.length - 1, Math.max(0, at < 0 ? 0 : at + step))];
    if (next === undefined) return;
    this.select(next, extend ? "range" : "single");
    this.#reveal(next);
  }

  /** Home | End. */
  moveTo(end: "first" | "last", extend: boolean): void {
    const keys = this.rows.filter((r) => r.type === "node").map(rowKey);
    const target = end === "first" ? keys[0] : keys[keys.length - 1];
    if (target === undefined) return;
    this.select(target, extend ? "range" : "single");
    this.#reveal(target);
  }

  /** Left | Right on the focused row: close | open a folder, or go to the parent | first child. */
  async arrow(direction: "left" | "right"): Promise<void> {
    const path = this.focus;
    if (path === null) return;
    const node = this.nodes.find((n) => n.path === path);
    if (node === undefined) return;
    const isDir = node.kind.type === "dir";
    if (direction === "right") {
      if (!isDir) return;
      if (!this.expanded.has(path)) await this.toggle(path);
      else this.move(1, false);
      return;
    }
    if (isDir && this.expanded.has(path)) {
      await this.toggle(path);
      return;
    }
    const parent = parentOf(path);
    if (parent !== "" && this.nodes.some((n) => n.path === parent)) {
      this.select(parent, "single");
      this.#reveal(parent);
    }
  }

  #reveal(path: string): void {
    this.#scrolls += 1;
    this.scrollTo = { path, n: this.#scrolls };
  }

  /** The filter shows hidden files (dot files | folders). */
  showsHidden(): boolean {
    return this.filter.showHidden;
  }

  /** Paths an action works on: the selection, else the focused row. */
  /** The one folder selected (`null`: nothing, a file, or several things). */
  selectedFolder(): string | null {
    if (this.selection.size !== 1) return null;
    const [path] = Array.from(this.selection);
    if (path === undefined) return null;
    return this.nodes.some((n) => n.path === path && n.kind.type === "dir") ? path : null;
  }

  targets(): string[] {
    if (this.selection.size > 0) return topmost(this.selection);
    return this.focus === null ? [] : [this.focus];
  }

  // ---- open / reveal ---------------------------------------------------------------------------------------------

  /** Enter | double click | click on a file: a folder toggles, a file is shown. `inList` false is for
   *  a path the backend has just created and has not listed yet (its row may still be on its way). */
  async activate(path: string, inList = true): Promise<void> {
    if (this.#closed) return;
    const node = this.nodes.find((n) => n.path === path);
    if (inList && node === undefined) return;
    if (node?.kind.type === "dir") {
      await this.toggle(path);
      return;
    }
    if (!(await confirmLeave({ kind: "document", path }))) {
      // the person keeps editing: the highlight goes back to the document still shown
      this.revealActive();
      return;
    }
    const result = await this.#api.showDocument(path);
    if (!result.ok) this.#host.fail(result.error);
  }

  /** The window shows this document: keep it in the list and bring it into view. */
  async setActive(path: string | null): Promise<void> {
    this.#active = path;
    if (this.load.status !== "ready") return;
    await this.#follow(path);
  }

  async #follow(path: string | null): Promise<void> {
    if (this.#closed) return;
    const kept = await this.#api.treeKeep(path);
    if (!kept.ok && !treeGone(kept.error)) this.#host.fail(kept.error);
    if (path !== null) this.reveal(path);
  }

  /** The document the window shows, brought into view. */
  revealActive(): void {
    if (this.#active !== null) this.reveal(this.#active);
  }

  /** Opens the folders above `path`, selects it, scrolls to it. A row the list does not have yet is
   *  kept as the path to select, and joins the selection when its delta arrives (as `placed` does). */
  reveal(path: string): void {
    void this.#openFolders(ancestorsOf(path));
    const root = rootOf(path);
    if (root !== "" && this.collapsedRoots.has(root)) {
      const roots = new Set(this.collapsedRoots);
      roots.delete(root);
      this.collapsedRoots = roots;
    }
    const alive = new Set(this.nodes.map((n) => n.path));
    if (alive.has(path)) {
      this.selection = new Set([path]);
      this.#anchor = path;
      this.focus = path;
    } else {
      this.#toSelect.add(path);
    }
    this.#reveal(path);
  }

  // ---- inline names ----------------------------------------------------------------------------------------------

  beginRename(path?: string): void {
    const target = path ?? this.targets()[0];
    if (target === undefined || this.targets().length > 1) return;
    this.edit = { mode: "rename", path: target, value: nameOf(target), problem: null };
    this.#reveal(target);
  }

  /** A new file | folder: the field opens inside `dir` (the folder is opened, at the store and at the
   *  backend, so the row that arrives can be selected). */
  beginNew(kind: NewKind, dir?: string): void {
    const base = dir ?? this.#folderForNew();
    if (base !== "") void this.#openFolders([base]);
    this.edit = {
      mode: "new",
      kind,
      dir: base,
      value: kind === "folder" ? t("fileOps.defaultFolderName") : "",
      problem: null,
    };
  }

  /** Where "new" goes with nothing chosen: the focused folder, the folder of the focused file, else the top. */
  #folderForNew(): string {
    const path = this.focus;
    if (path === null) return "";
    const node = this.nodes.find((n) => n.path === path);
    return node?.kind.type === "dir" ? path : parentOf(path);
  }

  typeName(value: string): void {
    if (this.edit.mode === "none") return;
    const problem = value === this.#original() ? null : quickNameProblem(value);
    this.edit = { ...this.edit, value, problem };
  }

  #original(): string {
    return this.edit.mode === "rename" ? nameOf(this.edit.path) : "";
  }

  cancelEdit(): void {
    this.edit = { mode: "none" };
  }

  async commitEdit(): Promise<void> {
    if (this.#closed) return;
    const edit = this.edit;
    if (edit.mode === "none") return;
    const problem = quickNameProblem(edit.value);
    if (edit.mode === "rename" && edit.value === nameOf(edit.path)) {
      this.cancelEdit();
      return;
    }
    if (problem !== null) {
      this.edit = { ...edit, problem };
      return;
    }
    if (edit.mode === "rename") {
      const parent = parentOf(edit.path);
      const to = parent === "" ? edit.value : `${parent}/${edit.value}`;
      // a refusal keeps the field open with the reason
      await this.requestMove([{ from: edit.path, to }], true);
      return;
    }
    const dir = edit.dir;
    const kind = edit.kind;
    const result = await this.#api.newEntry(dir, edit.value, kind);
    if (!result.ok) {
      this.#fieldFailure(result.error);
      return;
    }
    this.edit = { mode: "none" };
    const path = result.value;
    await this.#openFolders([dir]);
    this.reveal(path);
    // a new file is what the person wants to work on: it opens in the editor (a folder stays a row),
    // through the window's own show path and the mode the editing session applies when it arrives
    if (kind === "file") {
      editing.chooseNext("edit");
      await this.activate(path, false);
    }
  }

  /** A refusal about the name stays in the field; anything else is told to the window. */
  #fieldFailure(error: ApiError): void {
    if (this.edit.mode !== "none" && error.kind === "backend") {
      const kind = error.error.kind;
      if (kind === "invalidName") {
        const detail = error.error.detail;
        this.edit = { ...this.edit, problem: isNameProblem(detail) ? detail : "chars" };
        return;
      }
      if (kind === "exists") {
        const unknown = error.error.detail === IDENTITY_UNKNOWN;
        this.edit = { ...this.edit, problem: unknown ? "identityUnknown" : "exists" };
        return;
      }
    }
    this.cancelEdit();
    this.#host.fail(error);
  }

  // ---- moves -----------------------------------------------------------------------------------------------------

  /**
   * Plans a move and, by the link-update setting, asks | applies | applies without links. `inField`: the move comes
   * from the inline name field, which shows a refusal about the name.
   */
  async requestMove(moves: MoveSpec[], inField = false): Promise<void> {
    if (this.#closed) return;
    const setting = this.#host.linkUpdate();
    const plan = await this.#api.planMove(moves, setting !== "never");
    if (!plan.ok) {
      if (inField) this.#fieldFailure(plan.error);
      else this.#host.fail(plan.error);
      return;
    }
    const value = plan.value;
    const needsAnswer = setting === "ask" && (value.linkCount > 0 || value.skipped.length > 0);
    if (needsAnswer) {
      this.dialog = { status: "asking", plan: value };
      return;
    }
    await this.#apply(value, setting !== "never");
  }

  async answerDialog(choice: "update" | "move" | "cancel"): Promise<void> {
    const asked = this.dialog;
    this.dialog = { status: "closed" };
    if (asked.status !== "asking" || choice === "cancel") {
      if (asked.status === "asking") this.cancelEdit();
      return;
    }
    await this.#apply(asked.plan, choice === "update");
  }

  async #apply(plan: MovePlan, updateLinks: boolean): Promise<void> {
    if (this.#closed) return;
    const result = await this.#api.applyMove(plan.planId, updateLinks);
    this.edit = { mode: "none" };
    if (!result.ok) {
      this.#host.fail(result.error);
      return;
    }
    this.#host.note(this.#moveNote(plan, result.value), this.#undoAction());
    const last = plan.moves[plan.moves.length - 1];
    if (last !== undefined) this.reveal(last.to);
  }

  #moveNote(plan: MovePlan, outcome: MoveOutcome): string {
    const first = plan.moves[0];
    const base =
      plan.moves.length === 1 && first !== undefined
        ? parentOf(first.from) === parentOf(first.to)
          ? t("fileOps.summaryRenamed", { from: nameOf(first.from), to: nameOf(first.to) })
          : t("fileOps.summaryMoved", { what: nameOf(first.from) })
        : t("fileOps.summaryMoved", { what: tn("fileOps.items", plan.moves.length) });
    const parts = [base];
    if (outcome.links > 0) parts.push(tn("fileOps.linksUpdated", outcome.links));
    if (outcome.skipped.length > 0) parts.push(tn("fileOps.notUpdated", outcome.skipped.length));
    return parts.join(", ");
  }

  // ---- delete / undo ---------------------------------------------------------------------------------------------

  async remove(paths?: readonly string[]): Promise<void> {
    if (this.#closed) return;
    const targets = paths === undefined ? this.targets() : topmost(paths);
    if (targets.length === 0) return;
    const result = await this.#api.deleteFiles(targets);
    if (!result.ok) {
      this.#host.fail(result.error);
      return;
    }
    this.#host.note(
      targets.length === 1 && targets[0] !== undefined
        ? t("fileOps.summaryDeleted", { what: nameOf(targets[0]) })
        : t("fileOps.summaryDeleted", { what: tn("fileOps.items", targets.length) }),
      this.#undoAction(),
    );
  }

  /** The Undo of a note about a file operation: the journal's undo, as Ctrl+Z in the tree (wherever the focus is). */
  #undoAction(): NoticeAction {
    return { label: t("fileOps.undo"), run: () => void this.undo() };
  }

  async undo(): Promise<void> {
    if (this.#closed) return;
    this.#undone(await this.#api.undoFileOp(), t("fileOps.nothingToUndo"));
  }

  /** Takes back exactly the operation `id` (an import's notice): not the newest one, whatever came after. */
  async undoEntry(id: number): Promise<void> {
    if (this.#closed) return;
    this.#undone(await this.#api.undoFileOpEntry(id), t("import.undoGone"));
  }

  #undone(result: Awaited<ReturnType<Api["undoFileOp"]>>, none: string): void {
    if (!result.ok) {
      this.#host.fail(result.error);
      return;
    }
    if (result.value === null) {
      this.#host.note(none);
      return;
    }
    const left = result.value.skipped.length;
    this.#host.note(
      left === 0
        ? t("fileOps.undone")
        : `${t("fileOps.undone")} ${tn("fileOps.couldNotUndo", left)}`,
    );
  }

  // ---- outside PerMark -------------------------------------------------------------------------------------------

  async copyPath(path: string, full: boolean): Promise<void> {
    if (this.#closed) return;
    let text = path;
    if (full) {
      const abs = await this.#api.absPath(path);
      if (!abs.ok) {
        this.#host.fail(abs.error);
        return;
      }
      text = abs.value;
    }
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // the system refused the clipboard: nothing else to do
    }
  }

  /** "Copy reference": where the file lives, written with the person's template. */
  async copyReference(path: string): Promise<void> {
    const template = this.#host.referenceTemplate?.() ?? DEFAULT_REFERENCE_TEMPLATE;
    try {
      await navigator.clipboard.writeText(
        formatReference(template, { path, section: null, line: null }),
      );
    } catch {
      // the system refused the clipboard: nothing else to do
    }
  }

  /** Opens the file in the person's editor (setting). */
  async openInEditor(path: string): Promise<void> {
    if (this.#closed) return;
    const result = await this.#api.openInEditor(path, null, null);
    if (!result.ok) this.#host.fail(result.error);
  }

  async openExternal(path: string, mode: OpenMode): Promise<void> {
    if (this.#closed) return;
    const result = await this.#api.openExternal(path, mode);
    if (!result.ok) this.#host.fail(result.error);
  }

  async openAsProject(path: string): Promise<void> {
    if (this.#closed) return;
    const abs = await this.#api.absPath(path);
    if (!abs.ok) {
      this.#host.fail(abs.error);
      return;
    }
    this.#host.openProject(abs.value);
  }

  /** "Initialize PerMark in this folder": the window asks first, then creates `.permark/` there. */
  async askInit(path: string, name: string): Promise<void> {
    if (this.#closed) return;
    const abs = await this.#api.absPath(path);
    if (!abs.ok) {
      this.#host.fail(abs.error);
      return;
    }
    this.#host.initProject?.(abs.value, name);
  }

  // ---- drag ------------------------------------------------------------------------------------------------------

  /** The folder an outside drop at this point would land in, if the point is over the list. */
  dropFolderAt(x: number, y: number): string | null {
    return this.geometry?.(x, y) ?? null;
  }

  // ---- drag from outside -------------------------------------------------------------------------------------------

  /** An outside drag is over `folder` (`null`: over no folder of the list, or the drop cannot land here now) at
   *  `point` (`null`: not over the list). Resting on a closed folder opens it after a while. */
  outsideOver(folder: string | null, point: { x: number; y: number } | null): void {
    if (this.outside !== folder) this.outside = folder;
    this.outsidePoint = point;
    const wait = folder !== null && this.#isClosed(folder) ? folder : null;
    if (wait === this.#autoFor) return;
    this.#clearAuto();
    if (wait === null) return;
    this.#autoFor = wait;
    this.#autoTimer = setTimeout(() => {
      this.#autoTimer = null;
      this.#autoFor = null;
      void this.#autoOpen(wait);
    }, AUTO_EXPAND_MS);
  }

  /** The drag was dropped on the list: the mark goes, the folders it opened stay while the question is open. */
  outsideHold(): void {
    this.#clearAuto();
    this.outside = null;
    this.outsidePoint = null;
  }

  /** The drag is over: folders it opened close again, except `kept` (an import landed there) and those above it. */
  outsideEnd(kept: string | null): void {
    this.outsideHold();
    if (this.#autoOpened.size === 0) return;
    const expanded = new Set(this.expanded);
    const roots = new Set(this.collapsedRoots);
    for (const path of this.#autoOpened) {
      if (kept !== null && (kept === path || kept.startsWith(`${path}/`))) continue;
      if (this.roots.some((r) => r.path === path)) roots.add(path);
      else expanded.delete(path);
    }
    this.#autoOpened = new Set();
    this.expanded = expanded;
    this.collapsedRoots = roots;
  }

  /** Items an import put in place: their folders open, they are selected, the first comes into view (the keyboard
   *  focus stays where it is), and the open folders are remembered now. */
  placed(paths: readonly string[]): void {
    const first = paths[0];
    if (first === undefined) return;
    const expanded = new Set(this.expanded);
    for (const path of paths) for (const folder of ancestorsOf(path)) expanded.add(folder);
    this.expanded = expanded;
    const alive = new Set(this.nodes.map((n) => n.path));
    this.selection = new Set(paths.filter((p) => alive.has(p)));
    this.#toSelect = new Set(paths.filter((p) => !alive.has(p)));
    this.#anchor = first;
    this.#reveal(first);
    this.#remember();
  }

  /** Items that were placed before the list knew them join the selection when they arrive. */
  #selectArrived(alive: ReadonlySet<string>): void {
    if (this.#toSelect.size === 0) return;
    const arrived = Array.from(this.#toSelect).filter((p) => alive.has(p));
    const first = arrived[0];
    if (first === undefined) return;
    for (const path of arrived) this.#toSelect.delete(path);
    this.selection = new Set([...this.selection, ...arrived]);
    this.#anchor = first;
    this.#reveal(first);
  }

  /** A folder row that is closed now (a worktree root collapsed, or a folder not open). */
  #isClosed(path: string): boolean {
    if (path === "") return false;
    if (this.roots.some((r) => r.path === path)) return this.collapsedRoots.has(path);
    const node = this.nodes.find((n) => n.path === path);
    return node?.kind.type === "dir" && !this.expanded.has(path);
  }

  async #autoOpen(path: string): Promise<void> {
    if (!this.#isClosed(path)) return;
    this.#autoOpened.add(path);
    if (this.roots.some((r) => r.path === path)) {
      const roots = new Set(this.collapsedRoots);
      roots.delete(path);
      this.collapsedRoots = roots;
      return;
    }
    await this.#openFolders([path]);
  }

  #clearAuto(): void {
    if (this.#autoTimer !== null) clearTimeout(this.#autoTimer);
    this.#autoTimer = null;
    this.#autoFor = null;
  }

  beginDrag(paths: readonly string[]): void {
    this.drag = { status: "dragging", paths, over: null };
  }

  dragOver(target: string | null): void {
    if (this.drag.status !== "dragging") return;
    const over = target !== null && canDrop(this.drag.paths, target) ? target : null;
    if (over !== this.drag.over) this.drag = { ...this.drag, over };
  }

  cancelDrag(): void {
    this.drag = { status: "idle" };
  }

  async endDrag(): Promise<void> {
    const drag = this.drag;
    this.drag = { status: "idle" };
    if (drag.status !== "dragging" || drag.over === null) return;
    await this.requestMove(movesInto(drag.paths, drag.over));
  }
}
