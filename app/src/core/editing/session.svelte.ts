// The editing session of this window: the mode (View | Edit | Source), the document machine and the runner of its
// effects. The text lives in the editor on screen (`ActiveEditor`); the machine holds the version the edits are
// based on. One session per window; it holds at most one document.

import {
  api as defaultApi,
  type Api,
  type ApiError,
  type DocumentChanged,
  type EditSource,
  type Result,
  type Segment,
} from "../../api";
import type { Finder } from "../doc/find";
import { t } from "../i18n";
import {
  isUnsaved,
  transition,
  type Base,
  type DocEvent,
  type DocState,
  type Effect,
  type Leave,
  type SavePolicy,
} from "./docMachine";
import { setLeaveGuard } from "./guard";
import { lineEdits } from "./lines";

export type EditMode = "view" | "edit" | "source";

/** What Edit is for a document: the visual editor (Markdown), the editable sheet (config formats), or nothing
 * (other text: Source only). */
export type EditKind = "markdown" | "sheet" | "text";

/** What a whole-text edit works on: the selection, the Markdown block around the caret, or all of it. */
export type EditScope = "selection" | "paragraph" | "document";

/** A range of the edit text (UTF-16 offsets, `from <= to`). */
export type TextRange = { from: number; to: number };

/** Replace `from..to` with `insert`, only while that range still reads `expect`. */
export type TextEdit = TextRange & { insert: string; expect: string };

/** What an editor on screen offers the session. */
export type ActiveEditor = {
  kind: "edit" | "source";
  /** The whole edit text now. */
  text(): string;
  /** Blocks (Edit) | line runs (Source) that differ from `base` (save message). */
  changes(base: string): number;
  /** Line (1-based) at the top of the pane. */
  topLine(): number;
  /** The selected text ("" when none). */
  selection(): string;
  focus(): void;
  /** Puts pictures dropped at a point of the window into the text (the editors that take pictures). */
  pictures?(files: File[], point: { x: number; y: number }): void;
  /** The range `kind` covers now; `null`: none (empty selection, caret outside a prose block). */
  scope?(kind: EditScope): TextRange | null;
  /** All `edits` (ranges of the text now, not overlapping) in one undo step; `false` and no change when any range no
   *  longer reads its `expect`. Only `insert` goes in: nothing else of the text changes. */
  apply?(edits: readonly TextEdit[]): boolean;
  /** Selects `range` and scrolls it into view (Edit: the block scrolled into view when the range is not found in it). */
  reveal?(range: TextRange): void;
  /** Marks `ranges` in the text (decoration only), `current` distinct; a click on one -> `pick(its index)`. Marks follow
   *  edits until the next call; an empty list clears them. */
  marks?(ranges: readonly TextRange[], current: number | null, pick: (index: number) => void): void;
  /** Index (in the last `marks`) of the mark holding the caret; `null`: none. */
  markAt?(): number | null;
  /** Text is being composed (IME): edits wait. */
  composing?(): boolean;
  /** Some block of the text would read back otherwise than shown (a formula | link as text): a save says so. */
  lost?(): boolean;
  /** Find in page over the text (the page's find bar searches through it). */
  find?: Finder;
  /** Opens the editor's own find panel (Source). */
  search?(): void;
  /** Tops of the headings, px below the top edge of the text on screen, in document order (the outline). */
  headingTops?(): number[];
  /** Scrolls heading `index` (document order) to the top of the pane. */
  showHeading?(index: number): void;
};

/** The last `marks` handed to the editors. */
type Marks = {
  ranges: readonly TextRange[];
  current: number | null;
  pick: (index: number) => void;
};

/** What an editor mounts with. `line`: put this line at the top. */
export type Content = {
  path: string;
  text: string;
  segments: Segment[] | null;
  line: number;
  /** Bumped on every (re)load: the editor is made again. */
  seq: number;
  /** The editor takes the keyboard once made (`false`: it stays where the person put it, the Explorer's tree). */
  focus: boolean;
};

export type Hooks = {
  note(text: string): void;
  fail(error: ApiError): void;
  /** The document was saved: the reading view reads it again. */
  saved(): void;
  /** Opens a document in this window (a "save as new" result). */
  open(path: string): void;
  /** The reading view's top line, and back. */
  readingLine(): number;
  showReadingLine(line: number): void;
  /** Words for the save message (`first`: the first save of this window, with the hint). */
  savedText(changes: number, first: boolean): string;
};

const AUTOSAVE_MS = 800;
const CHECK_MS = 150;
/** How often the text the editor holds and the disk does not is handed to the journal. */
const JOURNAL_MS = 2000;

/** Keeps (`text`) or forgets (`null`) the unsaved text of a document: crash recovery. */
export type Journal = (path: string, baseHash: string, text: string | null) => void;

export class EditingSession {
  mode = $state<EditMode>("view");
  /** The last editing mode (Ctrl+E goes back to it). */
  lastEdit = $state<"edit" | "source">("edit");
  /** The mode the person chose in this window (View | Edit | Source). A file switch does not reset it, so the
   * next document opens in it; nothing is stored, so a new window starts in View. */
  chosenMode = $state<EditMode>("view");
  state = $state.raw<DocState>({ s: "closed" });
  content = $state.raw<Content | null>(null);
  /** The disk's version during a conflict. */
  disk = $state.raw<{ text: string; hash: string } | null>(null);
  /** The editor's text, kept fresh for measures (word count). */
  text = $state.raw("");
  selection = $state("");
  /** What Edit is here (`text`: no Edit mode, Source only). */
  kind = $state<EditKind>("text");
  /** First save of the session: the hint shows once. */
  firstSave = $state(true);
  /** Find in page over the editor on screen (`null`: none offers it; Source has its own panel). */
  finder = $state.raw<Finder | null>(null);

  #api: Api;
  #hooks: Hooks | null = null;
  #editor: ActiveEditor | null = null;
  #policy: SavePolicy = { kind: "explicit" };
  #path = $state<string | null>(null);
  /** Goes up when the session takes up another document or lets go of one, not when its document is renamed: an
   *  answer about an earlier document is dropped, one about the renamed document still counts. */
  #held = 0;
  /** An editor was opened in this window (Ctrl+E goes back to the last one). */
  #used = false;
  #seq = 0;
  #check: ReturnType<typeof setTimeout> | null = null;
  #autosave: ReturnType<typeof setTimeout> | null = null;
  #journal: Journal | null = null;
  #journalTimer: ReturnType<typeof setInterval> | null = null;
  /** This session handed text of the document to the journal (so it is the one to take it back). */
  #journaled = false;
  /** The window lets go of a document that may still hold unsaved text: its journal entry stays. */
  #keepJournal = false;
  #savedListeners = new Set<(path: string) => void>();
  #waiting: ((ok: boolean) => void)[] = [];
  #unguard: (() => void) | null = null;
  /** The block map the backend gave with the text it was made from: only that exact text may reuse it. */
  #loadedSegments: { text: string; segments: Segment[] } | null = null;
  /** Marks shown now: the editor made next (Edit <-> Source) shows them too. */
  #marks: Marks | null = null;
  /** The last save wrote a text that reads back otherwise (`ActiveEditor.lost`). */
  #lostSaved = false;

  constructor(api: Api = defaultApi) {
    this.#api = api;
  }

  bind(hooks: Hooks): void {
    this.#hooks = hooks;
  }

  /** The unsaved text is handed to `journal` every couple of seconds while it differs from the disk (`null`: stop). */
  bindJournal(journal: Journal | null): void {
    this.#journal = journal;
    if (journal === null) this.#stopJournal();
  }

  /** Hears every save of this window. Returns its removal. */
  onSaved(listener: (path: string) => void): () => void {
    this.#savedListeners.add(listener);
    return () => this.#savedListeners.delete(listener);
  }

  /** How the document being edited is saved. */
  get policy(): SavePolicy {
    return this.#policy;
  }

  get path(): string | null {
    return this.#path;
  }

  /** The editor Ctrl+E opens: the last one used here, else the setting. */
  lastEditOr(preference: "edit" | "source"): "edit" | "source" {
    return this.#used ? this.lastEdit : preference;
  }

  /** The mode the next document that opens in this window asks for (a file the person creates opens
   *  in the editor). Nothing is opened here: `openRemembered` in choose.ts applies it to the document
   *  that arrives, and the read-only gate keeps a file that cannot be edited in View. */
  chooseNext(mode: EditMode): void {
    this.chosenMode = mode;
  }

  get unsaved(): boolean {
    return isUnsaved(this.state);
  }

  get base(): Base | null {
    const s = this.state;
    switch (s.s) {
      case "clean":
      case "dirty":
      case "saving":
      case "conflict":
      case "saveError":
        return s.base;
      case "unsavedPrompt":
        return s.back.base;
      case "loading":
      case "closed":
        return null;
      default:
        return s satisfies never;
    }
  }

  // ---- entering and leaving ----------------------------------------------------------------------------------

  /** Opens `path` in an editor (`kind`: which Edit the document has; `at`: the line to start at, else the reading
   * place; `focus`: the editor takes the keyboard once made). */
  async enter(
    mode: "edit" | "source",
    path: string,
    kind: EditKind,
    policy: SavePolicy,
    at?: number,
    focus = true,
  ): Promise<void> {
    const wanted = kind === "text" ? "source" : mode;
    if (this.#path === path && this.mode !== "view") {
      await this.switchTo(wanted, at);
      return;
    }
    const line = at ?? this.#hooks?.readingLine() ?? 1;
    this.#reset();
    this.#path = path;
    this.#policy = policy;
    this.kind = kind;
    this.state = { s: "loading" };
    this.mode = wanted;
    this.lastEdit = wanted;
    // a file without Edit opens in Source, yet the person's choice stays: the next file that has Edit opens in it
    this.chosenMode = mode;
    this.#used = true;
    this.#unguard = setLeaveGuard((leave) => this.confirmLeave(leave));
    await this.#load(line, focus);
  }

  async #load(line: number, focus = true): Promise<void> {
    const path = this.#path;
    if (path === null) return;
    const seq = ++this.#seq;
    const result = await this.#api.editSource(path);
    if (seq !== this.#seq || this.#path !== path) return;
    if (!result.ok) {
      this.#hooks?.fail(result.error);
      this.#close();
      return;
    }
    this.#fitKind(result.value);
    this.#mount(result.value, line, focus);
    this.dispatch({ e: "loaded", base: { hash: result.value.hash, text: result.value.text } });
  }

  /**
   * The file, not the caller, decides between the visual editor and the others: the backend gives a block map for
   * Markdown only. The visual editor on other text, or the sheet on Markdown, would show (and save) it wrong.
   */
  #fitKind(source: EditSource): void {
    const markdown = source.segments !== null;
    if (markdown === (this.kind === "markdown")) return;
    if (markdown) {
      this.kind = "markdown";
      return;
    }
    this.kind = "text";
    this.mode = "source";
    this.lastEdit = "source";
  }

  #mount(source: EditSource, line: number, focus: boolean): void {
    this.#loadedSegments =
      source.segments === null ? null : { text: source.text, segments: source.segments };
    this.text = source.text;
    this.content = {
      path: source.path,
      text: source.text,
      segments: source.segments,
      line,
      seq: ++this.#seq,
      focus,
    };
  }

  /** Edit <-> Source with the text as it is now (`line` at the top); View asks first when there are unsaved edits. */
  async switchTo(mode: EditMode, at?: number): Promise<void> {
    if (mode === this.mode) return;
    if (mode === "view") {
      await this.confirmLeave({ kind: "view" });
      return;
    }
    const path = this.#path;
    if (path === null || this.content === null) return;
    // a reload is on its way: an editor made now would hold the text from before it, and the reload would be dropped
    if (this.state.s === "loading") return;
    if (mode === "edit" && this.kind === "text") return;
    const editor = this.#editor;
    const text = editor?.text() ?? this.content.text;
    const line = at ?? editor?.topLine() ?? this.content.line;
    let segments: Segment[] | null = null;
    if (mode === "edit" && this.kind === "markdown") {
      const loaded = this.#loadedSegments;
      if (loaded?.text === text) segments = loaded.segments;
      else {
        const seq = this.#seq;
        const result = await this.#api.editSegments(text);
        // the window moved on meanwhile (another document, a reload, another switch): this text is not its own now
        if (seq !== this.#seq || path !== this.#path) return;
        if (!result.ok) {
          this.#hooks?.fail(result.error);
          return;
        }
        segments = result.value;
      }
    }
    this.#editor = null;
    this.content = { path, text, segments, line, seq: ++this.#seq, focus: true };
    this.mode = mode;
    this.lastEdit = mode;
    this.chosenMode = mode;
  }

  /** Goes back to View once nothing is unsaved (scrolls the page to where the editor was). */
  #toView(): void {
    const line = this.#editor?.topLine() ?? 1;
    this.#close();
    this.chosenMode = "view";
    this.#hooks?.showReadingLine(line);
  }

  #reset(): void {
    this.#seq += 1;
    this.#held += 1;
    this.#editor = null;
    this.finder = null;
    this.content = null;
    this.disk = null;
    this.text = "";
    this.selection = "";
    this.#loadedSegments = null;
    this.#marks = null;
    this.#lostSaved = false;
    this.#clearTimers();
  }

  #close(): void {
    this.#stopJournal();
    if (!this.#keepJournal && this.#journaled && this.#path !== null && this.#journal !== null) {
      this.#journal(this.#path, this.base?.hash ?? "", null);
    }
    this.#journaled = false;
    this.#reset();
    this.#unguard?.();
    this.#unguard = null;
    this.#path = null;
    this.state = { s: "closed" };
    this.mode = "view";
    this.#settle(true);
  }

  /** The window lets go of the document (workspace gone): no question, nothing kept. */
  release(): void {
    if (this.unsaved) void this.#api.editSetUnsaved(null);
    this.#keepJournal = true;
    this.#close();
    this.#keepJournal = false;
  }

  // ---- the editor on screen ----------------------------------------------------------------------------------

  /** The editor of `seq` is on screen. Returns its removal. */
  attach(seq: number, editor: ActiveEditor): () => void {
    if (this.content?.seq !== seq) return () => {};
    this.#editor = editor;
    this.finder = editor.find ?? null;
    this.text = editor.text();
    const marks = this.#marks;
    if (marks !== null) editor.marks?.(marks.ranges, marks.current, marks.pick);
    return () => {
      if (this.#editor === editor) this.#editor = null;
      if (editor.find !== undefined && this.finder === editor.find) this.finder = null;
    };
  }

  /** The editor on screen takes pictures (a drop of pictures over it inserts them). */
  takesPictures(): boolean {
    return this.#editor?.pictures !== undefined;
  }

  /** Pictures dropped from the system at a point of the window go into the editor on screen. `false`: none takes them. */
  dropPictures(files: File[], point: { x: number; y: number }): boolean {
    const editor = this.#editor;
    if (!editor?.pictures) return false;
    editor.pictures(files, point);
    return true;
  }

  /** The text of the editor on screen now (`text` lags it by the change check); none: the last known text. */
  current(): string {
    return this.#editor?.text() ?? this.text;
  }

  /** The range `kind` covers in the editor on screen (`null`: none, or no editor offers it). */
  scope(kind: EditScope): TextRange | null {
    return this.#editor?.scope?.(kind) ?? null;
  }

  /** Puts `edits` into the editor on screen in one undo step. `false` (nothing changed): no editor takes them, it is
   *  composing text, or a range no longer reads what was expected. */
  apply(edits: readonly TextEdit[]): boolean {
    const editor = this.#editor;
    if (!editor?.apply || editor.composing?.() === true) return false;
    return editor.apply(edits);
  }

  /** Shows `range` in the editor on screen. */
  reveal(range: TextRange): void {
    this.#editor?.reveal?.(range);
  }

  /** Marks `ranges` in the editor on screen and in the next one made for this document (an empty list clears them). */
  marks(ranges: readonly TextRange[], current: number | null, pick: (index: number) => void): void {
    this.#marks = ranges.length === 0 ? null : { ranges, current, pick };
    this.#editor?.marks?.(ranges, current, pick);
  }

  /** Index of the mark holding the caret in the editor on screen (`null`: none, or no editor offers it). */
  markAt(): number | null {
    return this.#editor?.markAt?.() ?? null;
  }

  /** The editor on screen is composing text (IME). */
  composing(): boolean {
    return this.#editor?.composing?.() ?? false;
  }

  /** Gives the keyboard to the editor on screen (none: nothing happens). */
  focus(): void {
    this.#editor?.focus();
  }

  /** Opens the own find panel of the editor on screen (Source; others: nothing happens). */
  search(): void {
    this.#editor?.search?.();
  }

  /** Tops of the headings of the editor on screen (`[]`: none, or it does not tell). */
  headingTops(): number[] {
    return this.#editor?.headingTops?.() ?? [];
  }

  /** Scrolls the editor on screen to its heading `index` (one that cannot: nothing happens). */
  showHeading(index: number): void {
    this.#editor?.showHeading?.(index);
  }

  /** The editor's text changed (checked shortly after, against the base). */
  changed(): void {
    if (this.#check !== null) clearTimeout(this.#check);
    this.#check = setTimeout(() => {
      this.#check = null;
      this.#compare();
    }, CHECK_MS);
  }

  /** The selection changed. */
  selected(): void {
    this.selection = this.#editor?.selection() ?? "";
  }

  #compare(): void {
    const editor = this.#editor;
    const base = this.base;
    if (!editor || !base) return;
    const text = editor.text();
    this.text = text;
    this.dispatch(text === base.text ? { e: "reverted" } : { e: "edited" });
  }

  // ---- the machine -------------------------------------------------------------------------------------------

  dispatch(event: DocEvent): void {
    // a pending check first: "save" right after typing saves what was typed, and a change on disk right after typing
    // meets the typed text (a conflict), never a clean editor that reloads | closes over it
    if (
      event.e === "save" ||
      event.e === "leave" ||
      event.e === "saveAndLeave" ||
      event.e === "autosaveDue" ||
      event.e === "diskChanged" ||
      event.e === "diskDeleted"
    ) {
      this.sync();
    }
    const step = transition(this.state, event, this.#policy);
    this.state = step.state;
    // a save on the way out that failed | met a conflict: the person stays
    if (step.state.s === "saveError" || step.state.s === "conflict") this.#settle(false);
    for (const effect of step.effects) void this.#run(effect);
  }

  /** Runs the change check that waits at once: whoever reads `unsaved` next sees what was just typed. */
  sync(): void {
    if (this.#check !== null) {
      clearTimeout(this.#check);
      this.#check = null;
      this.#compare();
    }
  }

  /** The text being edited: the editor's, else the one the next editor mounts with, else the last one checked. */
  #textNow(): string {
    return this.#editor?.text() ?? this.content?.text ?? this.text;
  }

  save(): void {
    this.dispatch({ e: "save" });
  }

  async #run(effect: Effect): Promise<void> {
    switch (effect.type) {
      case "save":
        await this.#save(effect.against);
        return;
      case "saveAs":
        await this.#saveAs(effect.exact);
        return;
      case "load": {
        const line = this.#editor?.topLine() ?? this.content?.line ?? 1;
        this.disk = null;
        await this.#load(line);
        return;
      }
      case "fetchDisk": {
        const path = this.#path;
        if (path === null) return;
        const result = await this.#api.editSource(path);
        if (this.#path !== path) return;
        this.disk = result.ok ? { text: result.value.text, hash: result.value.hash } : null;
        return;
      }
      case "leave":
        this.#leave(effect.leave);
        return;
      case "setUnsaved":
        if (effect.unsaved) this.#startJournal();
        else this.#clearJournal();
        await this.#api.editSetUnsaved(effect.unsaved ? this.#path : null);
        return;
      case "scheduleAutosave":
        if (this.#autosave !== null) clearTimeout(this.#autosave);
        this.#autosave = setTimeout(() => {
          this.#autosave = null;
          this.dispatch({ e: "autosaveDue" });
        }, effect.delayMs || AUTOSAVE_MS);
        return;
      case "cancelAutosave":
        if (this.#autosave !== null) clearTimeout(this.#autosave);
        this.#autosave = null;
        return;
      default:
        effect satisfies never;
    }
  }

  async #save(against: "base" | "disk"): Promise<void> {
    const path = this.#path;
    const held = this.#held;
    const editor = this.#editor;
    const base = this.base;
    if (path === null || base === null) return;
    const text = this.#textNow();
    const lost = editor?.lost?.() ?? false;
    let from = base;
    if (against === "disk") {
      const disk = await this.#api.editSource(path);
      if (!disk.ok) {
        this.dispatch({ e: "saveFailed", error: disk.error.kind });
        return;
      }
      from = { hash: disk.value.hash, text: disk.value.text };
    }
    const changes = editor?.changes(from.text) ?? 0;
    const result = await this.#api.editSave(path, from.hash, lineEdits(from.text, text));
    // the answer belongs to this document even when it was renamed meanwhile (the machine waits for it)
    if (this.#held !== held) return;
    if (!result.ok) {
      this.#hooks?.fail(result.error);
      this.dispatch({ e: "saveFailed", error: result.error.kind });
      return;
    }
    const outcome = result.value;
    switch (outcome.outcome) {
      case "saved":
        // an autosave says nothing: it would speak every second
        if (this.#policy.kind === "explicit") {
          this.#hooks?.note(this.#hooks.savedText(changes, this.firstSave));
          this.firstSave = false;
        }
        this.#sayLost(lost);
        this.dispatch({ e: "saved", base: { hash: outcome.hash, text } });
        // the editor typed on while saving: compare against the new base
        if (editor && editor.text() !== text) this.changed();
        this.#hooks?.saved();
        for (const listener of this.#savedListeners) listener(path);
        return;
      case "conflict":
        this.dispatch({ e: "conflict", disk: outcome.diskHash });
        return;
      default:
        outcome satisfies never;
    }
  }

  async #saveAs(exact: boolean): Promise<void> {
    const path = this.#path;
    const held = this.#held;
    if (path === null) return;
    const lost = this.#editor?.lost?.() ?? false;
    const result = await this.#api.editSaveAs(path, this.#textNow(), exact);
    if (this.#held !== held) return;
    if (!result.ok) {
      this.#hooks?.fail(result.error);
      this.dispatch({ e: "saveFailed", error: result.error.kind });
      return;
    }
    this.#sayLost(lost);
    this.dispatch({ e: "savedAs", path: result.value, sameFile: exact });
    if (exact) this.#hooks?.saved();
  }

  /** The text just written reads back otherwise than the editor showed it: said on each explicit save, once while
   *  autosave keeps writing such a text (it would speak every second). */
  #sayLost(lost: boolean): void {
    if (lost && (this.#policy.kind === "explicit" || !this.#lostSaved)) {
      this.#hooks?.note(t("edit.savedReadsOtherwise"));
    }
    this.#lostSaved = lost;
  }

  #leave(leave: Leave): void {
    switch (leave.kind) {
      case "view":
        this.#toView();
        return;
      case "open": {
        this.#close();
        this.#hooks?.open(leave.path);
        return;
      }
      case "document":
      case "workspace":
      case "window":
        this.#close();
        return;
      default:
        leave satisfies never;
    }
  }

  #settle(ok: boolean): void {
    const waiting = this.#waiting;
    this.#waiting = [];
    for (const resolve of waiting) resolve(ok);
  }

  /** The leave guard: resolves `true` once the document is let go (saved or discarded), `false` to stay. */
  confirmLeave(leave: Leave): Promise<boolean> {
    if (this.state.s === "closed") return Promise.resolve(true);
    // the document being edited, asked again: nothing to leave
    if (leave.kind === "document" && leave.path === this.#path) return Promise.resolve(true);
    return new Promise((resolve) => {
      this.#waiting.push(resolve);
      this.dispatch({ e: "leave", leave });
    });
  }

  /**
   * The window closes and the person chose Save all: saves without a prompt, then lets go. `true` once saved (or
   * nothing to save), `false` when the save met a conflict | failed (the editor shows it; a conflict is never saved
   * over).
   */
  saveForClose(): Promise<boolean> {
    this.sync();
    if (!this.unsaved) return Promise.resolve(true);
    if (this.state.s === "conflict") return Promise.resolve(false);
    return new Promise((resolve) => {
      this.#waiting.push(resolve);
      this.dispatch({ e: "saveAndLeave", leave: { kind: "window" } });
    });
  }

  /** The person's answer in the unsaved prompt. */
  answer(choice: "save" | "discard" | "keepEditing"): void {
    if (choice === "keepEditing") {
      this.dispatch({ e: "keepEditing" });
      this.#settle(false);
      return;
    }
    this.dispatch({ e: choice });
  }

  // ---- the file on disk --------------------------------------------------------------------------------------

  /** `document-changed` for this window: the file changed or went away while it is edited. */
  onDocumentChanged(event: DocumentChanged): boolean {
    if (this.state.s === "closed" || event.path !== this.#path) return false;
    switch (event.change) {
      case "changed":
        this.dispatch({ e: "diskChanged", hash: event.version });
        return true;
      case "deleted":
        // what was just typed counts: the editor goes along with a renamed file; a deleted one, a clean editor lets go
        this.sync();
        if (event.movedTo !== null) void this.#follow(event.movedTo);
        else this.dispatch({ e: "diskDeleted" });
        return true;
      default:
        return event satisfies never;
    }
  }

  /**
   * The file was renamed | moved: the session goes with it (path; with unsaved edits, the unsaved flag and the journal
   * entry too), keeping the text and the mode; the editor is made again from it. The moved file is then compared with
   * the base like any change on disk: other bytes (links rewritten by the move) reload a clean editor, and show unsaved
   * edits the conflict view against the moved file, so "Keep mine" writes there and never brings the old path back.
   */
  async #follow(to: string): Promise<void> {
    const from = this.#path;
    const base = this.base;
    if (from === null || base === null) return;
    const text = this.#textNow();
    const line = this.#editor?.topLine() ?? this.content?.line ?? 1;
    this.#path = to;
    this.text = text;
    // nothing to type into until the text is mounted again for the new path
    this.#editor = null;
    this.content = null;
    if (this.#journaled && this.#journal !== null) {
      this.#journal(from, base.hash, null);
      this.#journaled = false;
      this.#writeJournal();
    }
    if (this.unsaved) void this.#api.editSetUnsaved(to);
    const seq = ++this.#seq;
    const loaded = this.#loadedSegments;
    const blocks: Result<Segment[]> | Promise<Result<Segment[]>> | null =
      this.mode !== "edit" || this.kind !== "markdown"
        ? null
        : loaded?.text === text
          ? { ok: true, value: loaded.segments }
          : this.#api.editSegments(text);
    const [map, disk] = await Promise.all([blocks, this.#api.editSource(to)]);
    if (seq !== this.#seq || this.#path !== to) return;
    if (map?.ok === false) {
      // Edit cannot show this text without its block map (it would rewrite blocks kept as written): Source shows it
      this.#hooks?.fail(map.error);
      this.mode = "source";
      this.lastEdit = "source";
    }
    const segments = map?.ok ? map.value : null;
    this.content = { path: to, text, segments, line, seq: ++this.#seq, focus: true };
    this.dispatch(disk.ok ? { e: "diskChanged", hash: disk.value.hash } : { e: "diskDeleted" });
  }

  // ---- the journal ---------------------------------------------------------------------------------------------

  #startJournal(): void {
    if (this.#journal === null || this.#journalTimer !== null) return;
    this.#journalTimer = setInterval(() => this.#writeJournal(), JOURNAL_MS);
  }

  #stopJournal(): void {
    if (this.#journalTimer !== null) clearInterval(this.#journalTimer);
    this.#journalTimer = null;
  }

  /** The text is on disk (or let go): the entry this session made goes. An entry an earlier run left stays until the
   * person chooses. */
  #clearJournal(): void {
    this.#stopJournal();
    if (this.#journaled && this.#path !== null) {
      this.#journal?.(this.#path, this.base?.hash ?? "", null);
    }
    this.#journaled = false;
  }

  #writeJournal(): void {
    const path = this.#path;
    const base = this.base;
    if (path === null || base === null || !this.unsaved) return;
    const text = this.#textNow();
    if (text === base.text || this.#journal === null) return;
    this.#journal(path, base.hash, text);
    this.#journaled = true;
  }

  #clearTimers(): void {
    if (this.#check !== null) clearTimeout(this.#check);
    if (this.#autosave !== null) clearTimeout(this.#autosave);
    this.#check = null;
    this.#autosave = null;
  }
}

export const editing = new EditingSession();
