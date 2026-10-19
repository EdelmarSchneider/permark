// The editing session of this window: the mode (View | Edit | Source), the document machine and the runner of its
// effects. The text lives in the editor on screen (`ActiveEditor`); the machine holds the version the edits are
// based on. One session per window; it holds at most one document.

import {
  api as defaultApi,
  type Api,
  type ApiError,
  type DocumentChanged,
  type EditSource,
  type Segment,
} from "../../api";
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
};

/** What an editor mounts with. `line`: put this line at the top. */
export type Content = {
  path: string;
  text: string;
  segments: Segment[] | null;
  line: number;
  /** Bumped on every (re)load: the editor is made again. */
  seq: number;
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

  #api: Api;
  #hooks: Hooks | null = null;
  #editor: ActiveEditor | null = null;
  #policy: SavePolicy = { kind: "explicit" };
  #path = $state<string | null>(null);
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
  #loadedSegments: Segment[] | null = null;

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
   * place). */
  async enter(
    mode: "edit" | "source",
    path: string,
    kind: EditKind,
    policy: SavePolicy,
    at?: number,
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
    this.#used = true;
    this.#unguard = setLeaveGuard((leave) => this.confirmLeave(leave));
    await this.#load(line);
  }

  async #load(line: number): Promise<void> {
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
    this.#mount(result.value, line);
    this.dispatch({ e: "loaded", base: { hash: result.value.hash, text: result.value.text } });
  }

  #mount(source: EditSource, line: number): void {
    this.#loadedSegments = source.segments;
    this.text = source.text;
    this.content = {
      path: source.path,
      text: source.text,
      segments: source.segments,
      line,
      seq: ++this.#seq,
    };
  }

  /** Edit <-> Source with the text as it is now (`line` at the top); View asks first when there are unsaved edits. */
  async switchTo(mode: EditMode, at?: number): Promise<void> {
    if (mode === this.mode) return;
    if (mode === "view") {
      await this.confirmLeave({ kind: "view" });
      return;
    }
    if (this.#path === null || this.content === null) return;
    if (mode === "edit" && this.kind === "text") return;
    const editor = this.#editor;
    const text = editor?.text() ?? this.content.text;
    const line = at ?? editor?.topLine() ?? this.content.line;
    let segments: Segment[] | null = null;
    if (mode === "edit" && this.kind === "markdown") {
      if (text === this.base?.text && this.#loadedSegments) segments = this.#loadedSegments;
      else {
        const result = await this.#api.editSegments(text);
        if (!result.ok) {
          this.#hooks?.fail(result.error);
          return;
        }
        segments = result.value;
      }
    }
    this.#editor = null;
    this.content = { path: this.#path, text, segments, line, seq: ++this.#seq };
    this.mode = mode;
    this.lastEdit = mode;
  }

  /** Goes back to View once nothing is unsaved (scrolls the page to where the editor was). */
  #toView(): void {
    const line = this.#editor?.topLine() ?? 1;
    this.#close();
    this.#hooks?.showReadingLine(line);
  }

  #reset(): void {
    this.#seq += 1;
    this.#editor = null;
    this.content = null;
    this.disk = null;
    this.text = "";
    this.selection = "";
    this.#loadedSegments = null;
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
    this.text = editor.text();
    return () => {
      if (this.#editor === editor) this.#editor = null;
    };
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
    // a pending check first: "save" right after typing saves what was typed
    if (event.e === "save" || event.e === "leave" || event.e === "autosaveDue") this.#flush();
    const step = transition(this.state, event, this.#policy);
    this.state = step.state;
    // a save on the way out that failed | met a conflict: the person stays
    if (step.state.s === "saveError" || step.state.s === "conflict") this.#settle(false);
    for (const effect of step.effects) void this.#run(effect);
  }

  #flush(): void {
    if (this.#check !== null) {
      clearTimeout(this.#check);
      this.#check = null;
      this.#compare();
    }
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
    const editor = this.#editor;
    const base = this.base;
    if (path === null || base === null) return;
    const text = editor?.text() ?? this.content?.text ?? base.text;
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
    if (this.#path !== path) return;
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
    if (path === null) return;
    const text = this.#editor?.text() ?? this.content?.text ?? "";
    const result = await this.#api.editSaveAs(path, text, exact);
    if (this.#path !== path) return;
    if (!result.ok) {
      this.#hooks?.fail(result.error);
      this.dispatch({ e: "saveFailed", error: result.error.kind });
      return;
    }
    this.dispatch({ e: "savedAs", path: result.value, sameFile: exact });
    if (exact) this.#hooks?.saved();
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
        this.dispatch({ e: "diskDeleted" });
        return true;
      default:
        return event satisfies never;
    }
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
    const text = this.#editor?.text() ?? this.content?.text ?? base.text;
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
