// The Notes window (docs/design/v0.5.md): the list of notes, the note on screen and everything done with them. The text of the
// open note is edited by the same editors as a document and saved by the edit store in its autosave mode (`edit.autosave`); this
// store decides which note is open, what happens when it is left, and keeps the list.

import { api } from "../api";
import { errorKind, errorMessage } from "../errors";
import { t, tn } from "../i18n";
import { isLoosePath, isPlainTextName } from "../looseFile";
import { countWords, isBlank } from "../noteText";
import {
  arrange,
  drawnOrder,
  dropPinned,
  filterNotes,
  movePinned,
  neighbour,
  pinnedAt,
  placeOf,
  type ArrangedNotes,
  type Frozen,
} from "../notesModel";
import {
  askYesNo,
  copyText,
  guardWindowClose,
  pickFile,
  pickSavePath,
  watchFileDrop,
} from "../platform";
import type {
  LooseStatus,
  NoteEntry,
  NoteMode,
  NoteVersion,
  NoteView,
  NotesFolder,
  NotesReadOnly,
  NotesRequest,
  PendingEdits,
} from "../types";
import { bridge } from "../bridge";
import { docs } from "./docs.svelte";
import { edit, type NoteWritten } from "./edit.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { vault } from "./vault.svelte";

/** The list is asked for again this long after the files in the folder changed. */
const REFRESH_DELAY_MS = 250;
/** The loose files are looked at this often while the window is in front, and at once when it gets the focus. */
const LOOSE_WATCH_MS = 2000;
/** The setting that reloads a loose file that changed on disk when nothing was typed into it (off unless the person turns it on). */
const AUTO_RELOAD_KEY = "trailmark.notes.autoReload";

function readAutoReload(): boolean {
  try {
    return localStorage.getItem(AUTO_RELOAD_KEY) === "1";
  } catch {
    return false;
  }
}
/** The text search behind the filter box waits for a pause in typing. */
const SEARCH_DELAY_MS = 180;
/** A new note starts in this editor. */
const NEW_NOTE_MODE: NoteMode = "visual";

/** The dialog of earlier versions: the list, the one selected, and its text. */
export type VersionsDialog = {
  path: string;
  items: NoteVersion[];
  selected: string | null;
  text: string | null;
  loading: boolean;
  failed: string | null;
};

/**
 * What the loose file on screen needs the person to decide: it changed on disk (what is shown stays until they choose), or it
 * is not there any more.
 */
export type LooseNotice = {
  path: string;
  kind: "changed" | "missing";
  /** How the file looked when this was found, so that "Keep mine" can be remembered for that version only. */
  stamp: string;
};

class NotesStore {
  /** "boot": starting. "missing": the notes folder is not there. "failed": it could not be opened. */
  phase = $state<"boot" | "missing" | "ready" | "failed">("boot");
  failure = $state<string | null>(null);
  folder = $state.raw<NotesFolder | null>(null);
  /** The notes can only be read here: another TrailMark window has them open (or the folder cannot be written). */
  readOnly = $state<NotesReadOnly | null>(null);
  entries = $state.raw<NoteEntry[]>([]);
  /** The note on screen. A new note has a name here before it has a file. */
  current = $state<string | null>(null);
  /** The note on screen is new and has no file yet: it gets one with its first text. */
  unborn = $state(false);
  filter = $state("");
  /** The notes whose text the search found for the filter (besides the ones whose title or snippet match). */
  found = $state.raw<ReadonlySet<string> | null>(null);
  /** The note being edited keeps its place in the list until it is left. */
  frozen = $state.raw<Frozen | null>(null);
  versions = $state.raw<VersionsDialog | null>(null);
  /** The header is asking for a new file name. */
  renaming = $state(false);
  /** Bumped to move the keyboard focus to the list (Ctrl+Shift+D) or to the filter box (Ctrl+F in the list). */
  listFocusTick = $state(0);
  filterFocusTick = $state(0);
  /** The word count and the length of the note as it was last saved or opened (the status bar). */
  words = $state(0);
  savedAtMs = $state<number | null>(null);
  /** The loose file on screen changed on disk or is gone, and waits for the person's choice. */
  looseNotice = $state.raw<LooseNotice | null>(null);
  /** The loose file on screen is not there, so nothing is shown of it: the text kept for it (if any) is what can be saved. */
  missingKept = $state.raw<PendingEdits | null>(null);
  /** Reload a loose file that changed on disk by itself when nothing was typed into it (Settings, Notes). Off by default. */
  autoReload = $state(readAutoReload());

  /** The note was blank when it was opened: if it still is when it is left, it was never written in. */
  private openedBlank = false;
  /** The versions of loose files whose change the person chose to ignore ("Keep mine" while reading): path to stamp. */
  private ignored = new Map<string, string>();
  private looseTimer: ReturnType<typeof setInterval> | null = null;
  private checkingLoose = false;
  private refreshTimer: ReturnType<typeof setTimeout> | null = null;
  private searchTimer: ReturnType<typeof setTimeout> | null = null;
  private searchToken = 0;
  private listToken = 0;
  private started = false;
  /** Opening one note at a time. */
  private switching = false;

  // ---- what the window shows -----------------------------------------------------------------------

  /** The notes as the list draws them: the filter applied, and a new note that has no file yet at the top. */
  shown = $derived.by<NoteEntry[]>(() => {
    const all =
      this.unborn && this.current
        ? [this.virtualEntry(this.current), ...this.entries]
        : this.entries;
    return filterNotes(all, this.filter, this.found);
  });

  /** The current note's entry (null for one that is not in the list yet, or none). */
  currentEntry = $derived(
    this.current ? (this.entries.find((n) => n.path === this.current) ?? null) : null,
  );

  /** The open row is a loose file: a file of the disk that is not in the notes folder. */
  isFile = $derived(this.currentEntry?.kind === "file" || isLoosePath(this.current));

  /** The loose file on screen is not there: there is no text to show, only what to do about it. */
  currentMissing = $derived(this.currentEntry?.kind === "file" && this.currentEntry.missing);

  /** The paths in the order the list draws them: the pinned ones, each group, then the files. */
  order(now = Date.now()): string[] {
    return drawnOrder(this.arrangement(now)).map((n) => n.path);
  }

  arrangement(now: number): ArrangedNotes {
    return arrange(this.shown, now, this.frozen);
  }

  private virtualEntry(path: string): NoteEntry {
    return {
      kind: "note",
      path,
      title: "",
      snippet: "",
      modifiedMs: this.frozen?.modifiedMs ?? Date.now(),
      sizeBytes: 0,
      pinned: false,
      copied: false,
      view: null,
      folder: null,
      missing: false,
      unsaved: false,
      shared: false,
    };
  }

  /** Forgets the list and the note on screen (the notes folder is about to be opened again). */
  reset(): void {
    if (this.refreshTimer !== null) clearTimeout(this.refreshTimer);
    if (this.searchTimer !== null) clearTimeout(this.searchTimer);
    this.refreshTimer = null;
    this.searchTimer = null;
    this.phase = "boot";
    this.failure = null;
    this.folder = null;
    this.readOnly = null;
    this.entries = [];
    this.current = null;
    this.unborn = false;
    this.filter = "";
    this.found = null;
    this.frozen = null;
    this.versions = null;
    this.renaming = false;
    this.words = 0;
    this.savedAtMs = null;
    this.looseNotice = null;
    this.missingKept = null;
    this.ignored.clear();
    this.openedBlank = false;
    this.switching = false;
  }

  // ---- starting ------------------------------------------------------------------------------------

  /** Boots the Notes window: the folder, the list, and the note it was left on (or a new one ready to type in). */
  async start(): Promise<void> {
    if (!this.started) {
      this.started = true;
      this.wire();
    }
    try {
      this.folder = await api.notesFolder();
    } catch (error) {
      this.fail(error);
      return;
    }
    if (this.folder.status === "missing") {
      this.phase = "missing";
      return;
    }
    await this.load();
  }

  /** Opens the folder that is there: the first time, and after the person located or created it. */
  private async load(): Promise<void> {
    this.phase = "boot";
    try {
      const opened = await api.openNotes();
      vault.reset();
      vault.info = opened.info;
      this.readOnly = opened.readOnly;
      edit.readOnlyReason = opened.readOnly ? t(`notes.readOnly.${opened.readOnly}`) : null;
      const [stored, tree, list] = await Promise.all([
        api.loadSession().catch(() => null),
        api.listTree(false, false),
        api.listNotes(),
      ]);
      session.load(stored);
      vault.tree = tree;
      this.entries = list.notes;
      this.phase = "ready";
      if (opened.recovered.length > 0) {
        toast.message(tn("notes.recovered", opened.recovered.length), "info");
      }
      await this.restore(list.open, await api.takeNotesRequest().catch(() => null));
    } catch (error) {
      this.fail(error);
    }
  }

  private fail(error: unknown): void {
    this.failure = errorMessage(error);
    this.phase = "failed";
  }

  /** What to show first: what another window asked for, else the note this window was left on, else a new note. */
  private async restore(open: string | null, request: NotesRequest | null): Promise<void> {
    if (request?.openFile && (await this.openFile(request.openFile))) return;
    if (request?.newNote) return this.newNote();
    if (request?.open && this.has(request.open)) return void (await this.openNote(request.open));
    if (open && this.has(open)) return void (await this.openNote(open));
    if (this.readOnly) return;
    return this.newNote();
  }

  private has(path: string): boolean {
    return this.entries.some((n) => n.path === path);
  }

  /** Window-wide listeners: what leaves the window flushes the note. Registered once. */
  private wire(): void {
    edit.autosave = true;
    // a plain text file of the disk is edited here, in Source
    edit.textFiles = (path) => isLoosePath(path) && isPlainTextName(path);
    edit.noteHooks = {
      written: (note) => this.written(note),
      renamed: (from, to) => this.renamedByTheSave(from, to),
      conflict: (copy) => this.keptAsCopy(copy),
      gone: (path) => void this.noteGone(path),
      pending: (path, kept) => this.keptApart(path, kept),
      looseConflict: (path, diskHash) => this.savedOverAnotherVersion(path, diskHash),
    };
    // Closing never asks: the note is saved first. The window has no per-edit guard, this one stays for good.
    void guardWindowClose(async () => {
      await this.leaveForClose();
      return edit.confirmLeave("close");
    });
    window.addEventListener("blur", () => void edit.flush());
    window.addEventListener("focus", () => void this.checkLoose());
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") void edit.flush();
    });
    // a file dropped on the window is opened: a loose file under Files (a note of the folder, as a note)
    void watchFileDrop({
      over: () => {},
      leave: () => {},
      drop: (paths) => void this.openFiles(paths),
    });
    // the loose files are looked at while the window is in front (and again at once when it comes back)
    this.looseTimer = setInterval(() => {
      if (document.visibilityState === "visible" && document.hasFocus()) void this.checkLoose();
    }, LOOSE_WATCH_MS);
  }

  /** The window is closing: where the note was left is remembered with the note. */
  private async leaveForClose(): Promise<void> {
    await edit.flush();
    this.rememberView();
  }

  // ---- the folder that is not there ----------------------------------------------------------------

  /** "Locate...": the person picked a folder for the notes. */
  async locate(path: string): Promise<void> {
    try {
      this.folder = await api.setNotesFolder(path);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      return;
    }
    await this.load();
  }

  /** "Create it": makes the folder that was configured but is not there. */
  async createFolder(): Promise<void> {
    try {
      this.folder = await api.createNotesFolder();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      return;
    }
    await this.load();
  }

  async openFolder(): Promise<void> {
    try {
      await api.openNotesFolder();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  // ---- the list ------------------------------------------------------------------------------------

  /** Reads the list again (the folder changed, or something was done to a note). */
  async refresh(): Promise<void> {
    const token = ++this.listToken;
    try {
      const list = await api.listNotes();
      if (token !== this.listToken) return;
      this.entries = list.notes;
    } catch {
      // the list keeps what it had; the next change asks again
    }
  }

  /** The files of the folder changed: the list is asked for again once the burst is over. */
  folderChanged(tree = true): void {
    if (this.refreshTimer !== null) clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = null;
      void this.refresh();
      if (tree) void vault.refreshTree();
    }, REFRESH_DELAY_MS);
  }

  /** The filter box changed: titles and snippets match at once, and the search of the texts follows a pause later. */
  setFilter(query: string): void {
    this.filter = query;
    if (this.searchTimer !== null) clearTimeout(this.searchTimer);
    const token = ++this.searchToken;
    if (query.trim() === "") {
      this.found = null;
      return;
    }
    this.searchTimer = setTimeout(async () => {
      this.searchTimer = null;
      try {
        const hits = await api.search(query, 60);
        if (token === this.searchToken) this.found = new Set(hits.map((hit) => hit.path));
      } catch {
        if (token === this.searchToken) this.found = null;
      }
    }, SEARCH_DELAY_MS);
  }

  clearFilter(): void {
    this.setFilter("");
  }

  focusList(): void {
    this.listFocusTick++;
  }

  focusFilter(): void {
    this.filterFocusTick++;
  }

  // ---- opening and leaving ------------------------------------------------------------------------

  /**
   * Gets the open note ready to be left: its text is saved (a note that cannot be saved is not left), where it was left is
   * remembered, a new note that was never written is forgotten and a blank note that was never written in is removed.
   * Resolves to whether the note was left.
   */
  private async leave(): Promise<boolean> {
    const path = this.current;
    if (!path) return true;
    if (!(await edit.flush())) return false;
    this.rememberView();
    if (this.unborn) {
      if (edit.active) edit.abandon();
      void api.forgetNewNote(path).catch(() => {});
    } else {
      const blank = this.openedBlank && edit.active && isBlank(edit.exactText());
      if (edit.active) await edit.leave({ refresh: false });
      if (blank) {
        // only a note that was blank when it was opened and still is: the person never wrote in it
        const removed = await api.discardBlankNote(path).catch(() => false);
        if (removed) await this.refresh();
      }
    }
    this.current = null;
    this.unborn = false;
    this.frozen = null;
    this.looseNotice = null;
    this.missingKept = null;
    // it moves up the list now that it is left
    void this.refresh();
    return true;
  }

  /** Remembers where the open note was left: the editor, the caret and the top of the view (source lines from 0). */
  private rememberView(): void {
    const path = this.current;
    if (!path || this.unborn) return;
    const view = this.viewNow();
    if (view) void api.setNoteView(path, view).catch(() => {});
  }

  private viewNow(): NoteView | null {
    if (edit.active && bridge.editor) {
      return {
        mode: edit.mode,
        caret: bridge.editor.caretLine(),
        scroll: bridge.editor.topLine(),
      };
    }
    if (docs.current) {
      const top = (bridge.topLine?.() ?? 1) - 1;
      return { mode: "read", caret: null, scroll: Math.max(0, top) };
    }
    return null;
  }

  /** Opens a note, leaving the one that is open. */
  async openNote(path: string): Promise<boolean> {
    if (this.phase !== "ready" || this.switching) return false;
    if (path === this.current) {
      if (edit.active) bridge.editor?.focus();
      return true;
    }
    this.switching = true;
    try {
      if (!(await this.leave())) return false;
      return await this.show(path, false);
    } finally {
      this.switching = false;
    }
  }

  /** Shows a note in the editor it was left in. A new note (`fresh`) has no file yet and starts in Visual with the caret ready. */
  private async show(path: string, fresh: boolean): Promise<boolean> {
    const entry = this.entries.find((n) => n.path === path) ?? null;
    if (entry?.kind === "file") return this.showFile(path, entry);
    const view = entry?.view ?? null;
    const mode: NoteMode = fresh ? NEW_NOTE_MODE : (view?.mode ?? "visual");
    this.current = path;
    this.unborn = fresh;
    this.openedBlank = fresh || entry?.title === "";
    this.frozen = { path, modifiedMs: entry?.modifiedMs ?? Date.now() };
    const opened = await docs.open(path, {
      locate:
        mode === "read" && view?.scroll != null
          ? { line: view.scroll + 1, find: null, tint: false, align: "top" }
          : null,
    });
    if (!opened) {
      this.current = null;
      this.unborn = false;
      this.frozen = null;
      return false;
    }
    if (!fresh) void api.noteOpened(path).catch(() => {});
    this.words = 0;
    this.savedAtMs = entry?.modifiedMs ?? null;
    if (mode !== "read" && !this.readOnly) {
      const at =
        view?.caret != null ? { caret: view.caret, top: view.scroll ?? view.caret } : undefined;
      await edit.enter(fresh, mode, at);
    }
    if (!fresh) this.words = await this.wordsOf(path);
    return true;
  }

  /**
   * Shows a loose file. It opens in Read, and is never written on its own. When text was typed into it and not saved (kept
   * apart from it), it opens in the editor with that text and the "unsaved" mark. A file that is not there shows what can be done.
   */
  private async showFile(path: string, entry: NoteEntry): Promise<boolean> {
    const view = entry.view;
    this.current = path;
    this.unborn = false;
    this.openedBlank = false;
    this.frozen = null;
    this.looseNotice = null;
    this.missingKept = null;
    this.words = 0;
    this.savedAtMs = null;
    const kept = entry.unsaved ? await api.loadPending(path).catch(() => null) : null;
    if (entry.missing) {
      // nothing to read: the text that was kept (if any) can still be saved as a note or written again
      docs.reset();
      this.missingKept = kept;
      this.looseNotice = { path, kind: "missing", stamp: "" };
      void api.noteOpened(path).catch(() => {});
      return true;
    }
    const opened = await docs.open(path, {
      locate:
        !kept && view?.mode === "read" && view.scroll != null
          ? { line: view.scroll + 1, find: null, tint: false, align: "top" }
          : null,
    });
    if (!opened) {
      this.current = null;
      return false;
    }
    void api.noteOpened(path).catch(() => {});
    if (kept && !this.readOnly) {
      // back to the editor the text was typed in, over the file as it is now
      const mode: NoteMode = view?.mode && view.mode !== "read" ? view.mode : "visual";
      const at =
        view?.caret != null ? { caret: view.caret, top: view.scroll ?? view.caret } : undefined;
      await edit.enter(false, mode, at, kept);
      // the file is not the version the text was typed over: that is the person's to decide
      if (edit.changedUnderKeptText) {
        this.looseNotice = { path, kind: "changed", stamp: "" };
      }
    }
    this.words = await this.wordsOf(path);
    return true;
  }

  /** How many words the note on disk (or in the editor, when it is being written) has. */
  private async wordsOf(path: string): Promise<number> {
    try {
      const text = edit.active ? edit.exactText() : (await api.documentSource(path)).text;
      return countWords(text);
    } catch {
      return 0;
    }
  }

  /** `Ctrl+Shift+N`: a new note, selected at the top of Today, with the caret ready. */
  async newNote(): Promise<void> {
    if (this.phase !== "ready") return;
    if (this.readOnly) {
      toast.message(t(`notes.readOnly.${this.readOnly}`), "warn");
      return;
    }
    if (this.switching) return;
    // a new note on top of a new note that has nothing in it: it is already what was asked for
    if (this.unborn && this.current && edit.active && isBlank(edit.exactText())) {
      bridge.editor?.focus();
      return;
    }
    this.switching = true;
    try {
      if (!(await this.leave())) return;
      let path: string;
      try {
        path = await api.newNote();
      } catch (error) {
        toast.message(errorMessage(error), "warn");
        return;
      }
      this.filter = "";
      this.found = null;
      await this.show(path, true);
    } finally {
      this.switching = false;
    }
  }

  /** Another window or launch asked for something: a new note, a note, or just to be shown (the window is up already). */
  async handleRequest(request: NotesRequest | null): Promise<void> {
    if (this.phase !== "ready" || !request) return;
    if (request.openFile && (await this.openFile(request.openFile))) return;
    if (request.newNote) await this.newNote();
    else if (request.open && this.has(request.open)) await this.openNote(request.open);
    else this.focusList();
  }

  /** `Ctrl+Tab` and `Ctrl+Shift+Tab`: the next or previous note of the list. */
  async step(direction: -1 | 1): Promise<void> {
    const path = neighbour(this.order(), this.current, direction);
    if (path !== null && path !== this.current) await this.openNote(path);
  }

  /** `Ctrl+1` to `Ctrl+9`: the pinned notes, in order. */
  async openPinned(n: number): Promise<void> {
    const target = pinnedAt(this.arrangement(Date.now()).pinned, n);
    if (target) await this.openNote(target.path);
  }

  // ---- what the edit store tells -------------------------------------------------------------------

  /** A note was written. */
  private written(note: NoteWritten): void {
    this.words = countWords(note.text);
    this.savedAtMs = Date.now();
    if (note.created) {
      this.unborn = false;
      this.openedBlank = false;
      void api.noteOpened(note.path).catch(() => {});
      void this.refresh();
      void vault.refreshTree();
      return;
    }
    // patch the row at once: the folder's own writes are not announced as news
    this.entries = this.entries.map((n) =>
      n.path === note.path
        ? { ...n, copied: false, modifiedMs: Date.now(), sizeBytes: note.text.length }
        : n,
    );
    // the title and the snippet come from the files
    this.folderChanged(false);
  }

  /** A new note's name was taken by the time it was saved: it has another name now. */
  private renamedByTheSave(from: string, to: string): void {
    if (this.current === from) this.current = to;
    if (this.frozen?.path === from) this.frozen = { ...this.frozen, path: to };
    void docs.followMoves([{ from, to }]);
  }

  /** The text could not be saved over the note because it changed elsewhere: it is in a note of its own. */
  private keptAsCopy(copy: string): void {
    toast.message(t("notes.conflict", { name: copy }), "info");
    void this.refresh();
  }

  /** The note on screen is gone from the folder (deleted by another program) and has nothing unsaved. */
  private async noteGone(path: string): Promise<void> {
    if (this.current !== path) return;
    // the neighbour is chosen while the note is still in the list: afterwards the list does not have it any more
    const next = this.nextAfter([path]);
    edit.abandon();
    this.current = null;
    this.unborn = false;
    this.frozen = null;
    await this.refresh();
    toast.message(t("notes.gone"), "info");
    await this.openNeighbourOrNew(next);
  }

  // ---- loose files ---------------------------------------------------------------------------------

  /** Opens a file of the disk: a note when it is one of the notes folder, else a loose file under Files. Resolves to whether it opened. */
  async openFile(path: string): Promise<boolean> {
    if (this.phase !== "ready") return false;
    if (this.guardWrite()) return false;
    let opened;
    try {
      opened = await api.openLooseFile(path);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      return false;
    }
    this.filter = "";
    this.found = null;
    await this.refresh();
    return this.openNote(opened.path);
  }

  /** Files were dropped on the window: each is opened, the last one stays on screen. */
  async openFiles(paths: readonly string[]): Promise<void> {
    for (const path of paths) await this.openFile(path);
  }

  /** `Ctrl+Shift+O` and "Open file...": the person picks a file. */
  async chooseFile(): Promise<void> {
    try {
      const path = await pickFile();
      if (path) await this.openFile(path);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  /** Text typed into a loose file is kept apart from it, or nothing is kept any more (the row says "unsaved"). */
  private keptApart(path: string, kept: boolean): void {
    this.entries = this.entries.map((n) => (n.path === path ? { ...n, unsaved: kept } : n));
  }

  /**
   * Looks at every loose file: which are not there (the list dims them), which were found again under another name in their
   * folder, and whether the one on screen changed or is gone. What is on screen is never replaced: the person decides.
   */
  async checkLoose(): Promise<void> {
    if (
      this.phase !== "ready" ||
      this.checkingLoose ||
      !this.entries.some((n) => n.kind === "file")
    ) {
      return;
    }
    this.checkingLoose = true;
    let statuses: LooseStatus[];
    try {
      statuses = await api.looseStatus();
    } catch {
      return;
    } finally {
      this.checkingLoose = false;
    }
    this.applyLoose(statuses);
  }

  private applyLoose(statuses: readonly LooseStatus[]): void {
    const renamed = statuses.filter((s) => s.was !== null);
    for (const status of renamed) if (status.was) this.followRename(status.was, status.path);
    const missing = new Map(statuses.map((s) => [s.path, s.state === "missing"]));
    const flips = (n: NoteEntry): boolean =>
      n.kind === "file" && missing.has(n.path) && missing.get(n.path) !== n.missing;
    if (renamed.length > 0) void this.refresh();
    else if (this.entries.some(flips)) {
      // the list dims the files that are gone, and shows the ones that came back
      this.entries = this.entries.map((n) =>
        flips(n) ? { ...n, missing: missing.get(n.path) ?? n.missing } : n,
      );
    }
    const path = this.current;
    const status = path ? statuses.find((s) => s.path === path) : undefined;
    if (!path || !status) return;
    const notice = this.looseNotice;
    if (status.state === "missing") {
      if (notice?.kind !== "missing") this.looseNotice = { path, kind: "missing", stamp: "" };
    } else if (notice?.kind === "missing" && this.currentEntry?.missing === false) {
      // it is back
      this.looseNotice = null;
    }
    if (status.state === "changed" && this.ignored.get(path) !== status.stamp) {
      if (!notice || notice.kind !== "changed") {
        this.looseNotice = { path, kind: "changed", stamp: status.stamp };
        // the setting reloads a file nothing was typed into, without asking; one with text typed into it always asks
        if (this.autoReload && !(edit.active && edit.dirty)) void this.reloadFile();
      }
    }
  }

  /** Turns on or off the reloading of a loose file that changed on disk when nothing was typed into it. */
  setAutoReload(on: boolean): void {
    this.autoReload = on;
    try {
      localStorage.setItem(AUTO_RELOAD_KEY, on ? "1" : "0");
    } catch {
      // the choice holds for this run
    }
  }

  /** The file on screen was found under another name: the window follows it. */
  private followRename(from: string, to: string): void {
    if (this.current !== from) return;
    this.current = to;
    if (this.frozen) this.frozen = { ...this.frozen, path: to };
    if (this.looseNotice) this.looseNotice = { ...this.looseNotice, path: to };
    const moves = [{ from, to }];
    edit.followMoves(moves);
    void docs.followMoves(moves);
  }

  /** A save of the open loose file found it is not the version the person started from: nothing was written. */
  private savedOverAnotherVersion(path: string, diskHash: string | null): void {
    if (this.current !== path) return;
    this.looseNotice = { path, kind: diskHash === null ? "missing" : "changed", stamp: "" };
  }

  /**
   * "Reload": the file's own version replaces what is shown. Text typed into it and not saved is kept first, as a note named
   * "{name} (my edits)", so that nothing typed is lost.
   */
  async reloadFile(): Promise<void> {
    const notice = this.looseNotice;
    const path = this.current;
    if (!notice || !path || notice.kind !== "changed") return;
    if (edit.active && edit.dirty) {
      try {
        const note = await api.keepEditsAsNote(path, edit.exactText());
        toast.message(t("notes.file.editsKept", { name: note }), "info");
      } catch (error) {
        toast.message(errorMessage(error), "warn");
        return;
      }
    }
    this.looseNotice = null;
    this.ignored.delete(path);
    if (edit.active) await edit.reloadTheirs(true);
    else await docs.refresh();
    this.words = await this.wordsOf(path);
    await this.refresh();
  }

  /**
   * "Keep mine": what is shown stays. It is not written now: the next save of the file asks once more before it replaces the
   * version on disk. While only reading, the change is simply left alone (until the file changes again).
   */
  async keepMine(): Promise<void> {
    const notice = this.looseNotice;
    const path = this.current;
    if (!notice || !path || notice.kind !== "changed") return;
    this.looseNotice = null;
    if (edit.active) {
      try {
        // reading it is what makes it the version that is overwritten, with the person's say
        const source = await api.documentSource(path);
        edit.keepMineOver(source.hash);
      } catch (error) {
        toast.message(errorMessage(error), "warn");
        this.looseNotice = notice;
      }
      return;
    }
    if (notice.stamp !== "") this.ignored.set(path, notice.stamp);
  }

  /** Takes a file out of the list; the file itself is not touched. Text typed into it and not saved asks first. */
  async removeFile(path: string): Promise<void> {
    if (this.guardWrite()) return;
    const entry = this.entries.find((n) => n.path === path && n.kind === "file");
    if (!entry) return;
    if (entry.unsaved || (path === this.current && edit.active && edit.dirty)) {
      const question = t("notes.file.removeUnsaved", { name: entry.title });
      if (!(await askYesNo(question, t("notes.file.remove"), t("common.cancel")))) return;
    }
    const wasOpen = this.current === path;
    const next = wasOpen ? this.nextAfter([path]) : null;
    if (wasOpen) {
      edit.abandon();
      this.current = null;
      this.looseNotice = null;
      this.missingKept = null;
      docs.reset();
    }
    try {
      await api.removeLooseFile(path);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      await this.refresh();
      return;
    }
    this.ignored.delete(path);
    await this.refresh();
    toast.message(t("notes.file.removed", { name: entry.title }), "info");
    if (wasOpen) await this.openNeighbourOrNew(next);
  }

  /** "Locate...": the person picks the file this one became. */
  async locateFile(path: string): Promise<void> {
    if (this.guardWrite()) return;
    let picked: string | null;
    try {
      picked = await pickFile();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      return;
    }
    if (!picked) return;
    try {
      const opened = await api.locateLooseFile(path, picked);
      await this.refresh();
      if (this.current === path) {
        this.current = null;
        this.looseNotice = null;
        this.missingKept = null;
      }
      await this.openNote(opened.path);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  /** "Show in folder": the file in the system's file manager. */
  async revealFile(path: string): Promise<void> {
    try {
      await api.revealInFolder(path);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  /** The text of the loose file that is not there: what is being edited, else what was kept for it. */
  private textOfMissing(): string | null {
    if (edit.active) return edit.exactText();
    return this.missingKept?.text ?? null;
  }

  /** "Save as note": the text of a file that is gone becomes a note, "{name} (my edits)". */
  async saveMissingAsNote(): Promise<void> {
    const path = this.current;
    const text = this.textOfMissing();
    if (!path || text === null || this.guardWrite()) return;
    try {
      const note = await api.keepEditsAsNote(path, text);
      await this.refresh();
      toast.message(t("notes.file.editsKept", { name: note }), "info");
      await this.openNote(note);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  /** "Save it again": the file that is gone is written where it was, with the text typed into it. */
  async saveMissingAgain(): Promise<void> {
    const path = this.current;
    if (!path || this.guardWrite()) return;
    if (edit.active) {
      await edit.saveAsNew();
    } else {
      const text = this.missingKept?.text;
      if (text === undefined) return;
      try {
        await api.createDocument(path, text);
      } catch (error) {
        toast.message(errorMessage(error), "warn");
        return;
      }
    }
    this.looseNotice = null;
    this.missingKept = null;
    await this.refresh();
    // shown again as the file it is
    if (!edit.active) {
      this.current = null;
      await this.openNote(path);
    }
  }

  // ---- pinning -------------------------------------------------------------------------------------

  async togglePin(path: string): Promise<void> {
    const entry = this.entries.find((n) => n.path === path);
    if (!entry || this.guardWrite()) return;
    try {
      await api.setNotePinned(path, !entry.pinned);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
    await this.refresh();
  }

  /** `Alt+Up` and `Alt+Down` on a pinned note. */
  async movePin(path: string, direction: -1 | 1): Promise<void> {
    if (this.guardWrite()) return;
    const order = this.entries.filter((n) => n.pinned).map((n) => n.path);
    await this.setPinOrder(movePinned(order, path, direction));
  }

  /** A pinned note was dropped on another. */
  async dropPin(path: string, target: string): Promise<void> {
    if (this.guardWrite()) return;
    const order = this.entries.filter((n) => n.pinned).map((n) => n.path);
    await this.setPinOrder(dropPinned(order, path, target));
  }

  private async setPinOrder(order: string[]): Promise<void> {
    // the list shows the new order at once; the backend keeps it
    const rank = new Map(order.map((path, at) => [path, at]));
    this.entries = [...this.entries].sort((a, b) => {
      const [x, y] = [rank.get(a.path), rank.get(b.path)];
      if (x !== undefined && y !== undefined) return x - y;
      return x !== undefined ? -1 : y !== undefined ? 1 : 0;
    });
    try {
      await api.setPinOrder(order);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
    await this.refresh();
  }

  private guardWrite(): boolean {
    if (!this.readOnly) return false;
    toast.message(t(`notes.readOnly.${this.readOnly}`), "warn");
    return true;
  }

  // ---- naming --------------------------------------------------------------------------------------

  /** "Rename": the note gets a real file name. Resolves to the problem to show, or null when it worked. */
  async rename(path: string, name: string): Promise<string | null> {
    if (this.guardWrite()) return null;
    const typed = name.trim();
    if (typed === "") return t("notes.rename.empty");
    if (path === this.current && !(await edit.flush())) return t("notes.rename.notSaved");
    let to: string;
    try {
      to = await api.renameNote(path, typed);
    } catch (error) {
      return errorKind(error) === "exists"
        ? t("notes.rename.exists", { name: typed })
        : errorMessage(error);
    }
    if (to !== path) {
      const moves = [{ from: path, to }];
      if (this.current === path) {
        this.current = to;
        if (this.frozen) this.frozen = { ...this.frozen, path: to };
        edit.followMoves(moves);
        await docs.followMoves(moves);
      }
    }
    await this.refresh();
    void vault.refreshTree();
    return null;
  }

  // ---- copying -------------------------------------------------------------------------------------

  /** `Ctrl+Shift+C` and the Copy button: the note's text exactly as the file holds it, as plain text. */
  async copyNote(path = this.current): Promise<void> {
    if (!path) return;
    let text: string;
    try {
      if (path === this.current && edit.active) {
        await edit.flush();
        text = edit.exactText();
      } else {
        text = (await api.documentSource(path)).text;
      }
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      return;
    }
    if (isBlank(text)) {
      toast.message(t("notes.copyEmpty"), "info");
      return;
    }
    if (!(await copyText(text))) {
      toast.message(t("copy.failed"), "warn");
      return;
    }
    const length = Array.from(text).length;
    // only notes are marked as copied and offered for deleting: a loose file is somebody's file
    const entry = this.entries.find((n) => n.path === path && n.kind === "note");
    if (entry) {
      void api
        .markNoteCopied(path)
        .then(() => this.refresh())
        .catch(() => {});
      this.entries = this.entries.map((n) => (n.path === path ? { ...n, copied: true } : n));
    }
    const message = tn("notes.copied", length);
    if (entry && !entry.pinned && !this.readOnly) {
      toast.message(message, "info", {
        label: t("notes.deleteNote"),
        run: () => void this.deleteNotes([path]),
      });
    } else {
      toast.message(message, "info");
    }
  }

  // ---- deleting ------------------------------------------------------------------------------------

  /** Deletes notes (to the recycle bin), with Undo in the toast. A pinned note asks first. */
  async deleteNotes(paths: string[]): Promise<void> {
    if (this.guardWrite()) return;
    // a loose file is taken out of the list, not deleted: the file is not ours
    const files = paths.filter((p) => this.entries.some((n) => n.path === p && n.kind === "file"));
    for (const file of files) await this.removeFile(file);
    const known = paths.filter((p) => !files.includes(p) && this.entries.some((n) => n.path === p));
    if (known.length === 0) return;
    const pinned = this.entries.filter((n) => known.includes(n.path) && n.pinned);
    if (pinned.length > 0) {
      const name = pinned[0].title || t("notes.untitled");
      const question =
        pinned.length === 1
          ? t("notes.deletePinned", { name })
          : tn("notes.deletePinnedMany", pinned.length);
      if (!(await askYesNo(question, t("notes.delete"), t("common.cancel")))) return;
    }
    // the open note goes first: its autosave must not write it back. What was typed into it is saved before (Undo brings back
    // the latest text, not an older one); a note that cannot be saved is not deleted.
    const wasOpen = this.current !== null && known.includes(this.current);
    if (wasOpen && !(await edit.flush())) {
      toast.message(t("notes.delete.notSaved"), "warn");
      return;
    }
    const next = wasOpen ? this.nextAfter(known) : null;
    if (wasOpen) {
      edit.abandon();
      this.current = null;
      this.unborn = false;
      this.frozen = null;
    }
    // the name is read now: the list will not have the note any more
    const name = this.titleOf(known[0]);
    let outcome;
    try {
      outcome = await api.deleteNotes(known);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      await this.refresh();
      return;
    }
    await this.refresh();
    void vault.refreshTree();
    const undoId = outcome.undoId;
    toast.message(
      tn("notes.deleted", outcome.deleted.length, { name }),
      "info",
      undoId === null
        ? undefined
        : { label: t("notes.undo"), run: () => void this.undoDelete(undoId) },
    );
    if (wasOpen) await this.openNeighbourOrNew(next);
  }

  /** The title of a note that may no longer be in the list. */
  private titleOf(path: string): string {
    return this.entries.find((n) => n.path === path)?.title || t("notes.untitled");
  }

  /** The note the list moves to when `gone` are deleted: the one that follows the open note, else the one before it. */
  private nextAfter(gone: string[]): string | null {
    const order = this.order().filter((p) => p === this.current || !gone.includes(p));
    const at = this.current ? order.indexOf(this.current) : -1;
    return order[at + 1] ?? order[at - 1] ?? null;
  }

  private async openNeighbourOrNew(path: string | null): Promise<void> {
    if (path && this.has(path)) await this.openNote(path);
    else await this.newNote();
  }

  async undoDelete(undoId: number): Promise<void> {
    try {
      const back = await api.undoDeleteNotes(undoId);
      await this.refresh();
      void vault.refreshTree();
      if (back[0]) await this.openNote(back[0]);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  /** "Delete copied notes": every note that was copied and not edited since, but never a pinned one. */
  async deleteCopied(): Promise<void> {
    if (this.guardWrite()) return;
    // What was typed into the open note is saved first. If it was edited after it was copied, saving it takes the "copied" mark off
    // (so the list below no longer has it, and it is not deleted); if not, it is deleted with the others, and Undo brings it back as
    // it was typed.
    if (this.entries.some((n) => n.path === this.current && n.copied) && !(await edit.flush())) {
      toast.message(t("notes.delete.notSaved"), "warn");
      return;
    }
    const copied = this.entries.filter((n) => n.copied && !n.pinned).map((n) => n.path);
    if (copied.length === 0) return;
    const wasOpen = this.current !== null && copied.includes(this.current);
    const next = wasOpen ? this.nextAfter(copied) : null;
    if (wasOpen) {
      edit.abandon();
      this.current = null;
      this.unborn = false;
      this.frozen = null;
    }
    let outcome;
    try {
      outcome = await api.deleteCopiedNotes();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      await this.refresh();
      return;
    }
    await this.refresh();
    void vault.refreshTree();
    const undoId = outcome.undoId;
    toast.message(
      tn("notes.deletedCopied", outcome.deleted.length),
      "info",
      undoId === null
        ? undefined
        : { label: t("notes.undo"), run: () => void this.undoDelete(undoId) },
    );
    if (wasOpen) await this.openNeighbourOrNew(next);
  }

  /** How many notes are marked as copied (the list menu says "Delete copied notes (6)"; pinned notes are not counted). */
  get copiedCount(): number {
    return this.entries.filter((n) => n.copied && !n.pinned).length;
  }

  // ---- sharing and moving -------------------------------------------------------------------------

  /**
   * The open note is one that can be moved into a project or shared with agents: a note of the folder that has a file, in a
   * window that can write. A new note with no file yet, a loose file and a note that is not there are not.
   */
  get movable(): boolean {
    const entry = this.currentEntry;
    return (
      this.current !== null &&
      !this.unborn &&
      this.readOnly === null &&
      entry?.kind === "note" &&
      !isLoosePath(this.current)
    );
  }

  /** "Share with agents" and "Stop sharing": the note is marked, and an agent connected to a project can list and read it. */
  async toggleShare(path = this.current): Promise<void> {
    if (!path || this.guardWrite()) return;
    const entry = this.entries.find((n) => n.path === path && n.kind === "note");
    if (!entry || (path === this.current && this.unborn)) return;
    const shared = !entry.shared;
    try {
      await api.setNoteShared(path, shared);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      return;
    }
    this.entries = this.entries.map((n) => (n.path === path ? { ...n, shared } : n));
    toast.message(t(shared ? "notes.sharedOn" : "notes.sharedOff", { name: this.titleOf(path) }));
    await this.refresh();
  }

  /**
   * "Move to project...": asks where in the Save dialog (it opens in the project opened last, with a name made from the title),
   * writes the note there as it is, sends the note to the recycle bin and lists the copy under Files. The toast offers to open the
   * project on the copy and to undo it.
   */
  async moveToProject(path = this.current): Promise<void> {
    if (!path || this.guardWrite()) return;
    const entry = this.entries.find((n) => n.path === path && n.kind === "note");
    if (!entry || (path === this.current && this.unborn)) return;
    // what was typed is saved first: the copy is the note as it is now
    if (path === this.current && !(await edit.flush())) {
      toast.message(t("notes.move.notSaved"), "warn");
      return;
    }
    let destination: string | null;
    try {
      const suggestion = await api.moveSuggestion(path);
      destination = await pickSavePath(suggestion.defaultPath);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      return;
    }
    if (destination === null) return;
    // the name is read now: the list will not have the note any more
    const name = this.titleOf(path);
    const wasOpen = path === this.current;
    const next = wasOpen ? this.nextAfter([path]) : null;
    // the open note goes first: its autosave must not write it back
    if (wasOpen) {
      edit.abandon();
      this.current = null;
      this.unborn = false;
      this.frozen = null;
    }
    let moved;
    try {
      moved = await api.moveNoteToProject(path, destination);
    } catch (error) {
      const place = placeOf(destination);
      toast.message(
        errorKind(error) === "exists"
          ? t("notes.move.exists", { name: place.file })
          : errorMessage(error),
        "warn",
      );
      // nothing moved: the note is open again as it was
      await this.refresh();
      if (wasOpen) await this.openNote(path);
      return;
    }
    await this.refresh();
    void vault.refreshTree();
    const { undoId } = moved;
    toast.message(
      t("notes.moved", { name, folder: placeOf(destination).folder }),
      "info",
      { label: t("notes.moved.open"), run: () => void this.openInProject(destination) },
      undoId === null
        ? undefined
        : { label: t("notes.undo"), run: () => void this.undoMove(undoId, name, moved.file) },
    );
    if (wasOpen) await this.openNeighbourOrNew(next);
  }

  /** Starts TrailMark on the project that holds a file, showing it. */
  private async openInProject(file: string): Promise<void> {
    try {
      await api.openInProject(file);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  /**
   * Undo of a move: the note is back, and the copy in the project goes unless it was changed. Text typed into the copy and not
   * saved counts as a change: it is kept apart first (when the copy is the file on screen), and the copy then stays.
   */
  private async undoMove(undoId: number, name: string, copy: string): Promise<void> {
    try {
      if (this.current === copy && edit.active) await edit.flush();
      const undone = await api.undoMoveNote(undoId);
      await this.refresh();
      void vault.refreshTree();
      toast.message(t(undone.removed ? "notes.moveUndone" : "notes.moveUndoneKept", { name }));
      if (undone.note) await this.openNote(undone.note);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  // ---- earlier versions ----------------------------------------------------------------------------

  /** "Show earlier versions": a dialog that lists them and restores one. */
  async showVersions(path = this.current): Promise<void> {
    if (!path || this.unborn) return;
    // the text the note has now is one of its versions: it is saved before the list is read
    if (path === this.current && !(await edit.flush())) {
      toast.message(t("notes.versions.notSaved"), "warn");
      return;
    }
    this.versions = { path, items: [], selected: null, text: null, loading: true, failed: null };
    try {
      const items = await api.noteVersions(path);
      this.versions = { path, items, selected: null, text: null, loading: false, failed: null };
      if (items[0]) await this.selectVersion(items[0].id);
    } catch (error) {
      this.versions = {
        path,
        items: [],
        selected: null,
        text: null,
        loading: false,
        failed: errorMessage(error),
      };
    }
  }

  async selectVersion(id: string): Promise<void> {
    const dialog = this.versions;
    if (!dialog) return;
    this.versions = { ...dialog, selected: id, text: null, loading: true };
    try {
      const text = await api.noteVersionText(dialog.path, id);
      if (this.versions?.selected === id)
        this.versions = { ...this.versions, text, loading: false };
    } catch (error) {
      if (this.versions?.selected === id) {
        this.versions = { ...this.versions, loading: false, failed: errorMessage(error) };
      }
    }
  }

  closeVersions(): void {
    this.versions = null;
  }

  /** Puts the selected version back as the note's text. The text the note has now becomes a version first. */
  async restoreVersion(): Promise<void> {
    const dialog = this.versions;
    if (!dialog?.selected || this.guardWrite()) return;
    const { path, selected } = dialog;
    try {
      // what was typed is saved first (it becomes a version): the restore reloads the editor, and would overwrite text that was not
      if (path === this.current && !(await edit.flush())) {
        this.versions = { ...dialog, failed: t("notes.versions.notSaved") };
        return;
      }
      await api.restoreNoteVersion(path, selected);
    } catch (error) {
      this.versions = { ...dialog, failed: errorMessage(error) };
      return;
    }
    this.versions = null;
    if (path === this.current) {
      if (edit.active) await edit.reloadTheirs(true);
      else await docs.refresh();
    }
    await this.refresh();
    toast.message(t("notes.restored"), "info");
  }
}

export const notes = new NotesStore();
