// Editing the open document, in the Visual or the Source editor (docs/design/v0.2.md section 3). This store
// holds the version of the file the editor started from (`base`: text, hash and block map) and decides what
// happens at each step: entering and leaving (the Read | Visual | Source switch of the header, Ctrl+E, E and
// Ctrl+Alt+E), switching between the two editors, saving, the unsaved-changes bar, and what to do when the
// file changes on disk meanwhile. The text itself lives in the editor (reached through `bridge.editor`): Source
// holds a buffer, Visual holds a document that writes the text on demand.

import { tick } from "svelte";
import { api } from "../api";
import { Autosaver } from "../autosave";
import { bridge, type EditorApi } from "../bridge";
import {
  blockAtLine,
  buildBlockMap,
  changedBlockCount,
  joinBlockMap,
  mapLine,
  matchBlocks,
  type BlockMap,
} from "../editor/blockModel";
import { fromBuffer, toBuffer, type TextFormat } from "../editor/bufferText";
import type { VisualInput } from "../editor/visual/editor";
import { EDIT_BLOCK_TEXT, editBlock, editBlockFromKind } from "../editor/editability";
import {
  CONFIRM_ABOVE_BYTES,
  MAX_IMAGE_BYTES,
  imageExtension,
  isImageName,
  megabytes,
  pastedImageName,
  toBase64,
} from "../editor/imageName";
import { otherEditMode, type DocMode, type EditMode } from "../editMode";
import { errorKind, errorMessage } from "../errors";
import { mapPath } from "../fileOps";
import { t, tn } from "../i18n";
import { isLoosePath } from "../looseFile";
import { registerOverlay } from "../overlays";
import { basename } from "../paths";
import { askUnsaved, askYesNo, copyText, guardWindowClose, setWindowTitle } from "../platform";
import type {
  BufferEdits,
  BufferText,
  FsBatch,
  Move,
  ParsedBlocks,
  PendingEdits,
  SourceDoc,
} from "../types";
import { clock as formatClock } from "../time";
import { docs } from "./docs.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { vault } from "./vault.svelte";

/** More dirty blocks than this ask before "Reload theirs" throws them away. */
const CONFIRM_RELOAD_BLOCKS = 5;
/** A change of the file is looked at once the burst of events is over. */
const DISK_CHECK_DELAY_MS = 250;
/** "Saved, 2 blocks changed" stays this long in the header. */
const NOTE_MS = 3000;
/** A save of a note that takes longer than this says "Saving..." (a quick one just says "Saved"). */
const SLOW_SAVE_MS = 1000;
const HINT_KEY = "trailmark.hint.firstSave";

type Base = {
  text: string;
  /** The text as the editor holds it: no BOM, LF line endings. */
  buffer: string;
  format: TextFormat;
  hash: string;
  map: BlockMap;
  modifiedMs: number;
};

/** The file on disk is not the version being edited. */
export type Conflict = {
  atMs: number;
  /** The file is gone. */
  deleted: boolean;
  diskHash: string | null;
  /** Set when "Reload theirs" would throw away this many blocks of work and asks first. */
  confirmReload: number | null;
};

/** What the Visual editor starts from: the blocks of the text, which of them differ from the saved file, where the caret goes. */
export type VisualStart = VisualInput;

/** The "Unsaved changes" bar: what the person was doing, and how to carry on after Save or Discard. */
export type UnsavedPrompt = { reason: "exit" | "navigate"; resume: (() => void) | null };

/** Where the editor starts in a document: the line of the caret and the line at the top of the view (both from 0). */
export type EnterAt = { caret: number; top: number };

/** What the notes are told when a note was written. */
export type NoteWritten = {
  path: string;
  /** The file did not exist before this save. */
  created: boolean;
  text: string;
};

function readHintSeen(): boolean {
  try {
    return localStorage.getItem(HINT_KEY) === "1";
  } catch {
    return false;
  }
}

class EditStore {
  /** The document being edited; null while reading. */
  path = $state<string | null>(null);
  loading = $state(false);
  dirty = $state(false);
  saving = $state(false);
  saveError = $state<string | null>(null);
  conflict = $state.raw<Conflict | null>(null);
  prompt = $state.raw<UnsavedPrompt | null>(null);
  /** A short message in the header ("Saved, 2 blocks changed") that goes away by itself. */
  note = $state<string | null>(null);
  /** The one-time "TrailMark rewrites only the blocks you changed" bar. */
  hint = $state(false);
  caret = $state({ line: 1, column: 1 });
  /** When the file was last written, as far as the editor knows. */
  modifiedMs = $state(0);
  /** The editor in use. Entering starts in the editor used last (else the session's setting); the switch in the header changes it. */
  mode = $state<EditMode>("visual");
  /** The mode is being switched (Source to Visual waits for the blocks of the text), so another switch waits. */
  switching = $state(false);
  /** How many blocks carry the unsaved-block bar in Visual mode. */
  unsavedBlocks = $state(0);
  /**
   * What the Source editor opens with (text and line), read once when it mounts. `top` is the line to put at the top of the view
   * (the place the reading view or the Visual editor was showing); without it the caret's line is only made visible.
   */
  initial = $state.raw<{ buffer: string; line: number; atEnd: boolean; top?: number } | null>(null);
  /** What the Visual editor opens with, read once when it mounts. */
  visualStart = $state.raw<VisualStart | null>(null);
  /** Bumped for the bars to move the keyboard focus (Ctrl+S during a conflict). */
  conflictFocus = $state(0);
  /**
   * The Notes window: the text is saved as it is typed (docs/design/v0.5.md section 4), nothing asks "Save changes?", and a
   * note that changed on disk meanwhile is never overwritten (the text is kept as a copy).
   */
  autosave = $state(false);
  /** A save of a note has taken longer than a second: the status bar says "Saving...". */
  savingSlow = $state(false);
  /** Why nothing can be edited here (the notes of a second window can only be read), worded for the person. */
  readOnlyReason = $state<string | null>(null);
  /** The notes hear of each save, of a note that got another name, and of a text kept as a copy. */
  noteHooks: {
    written?: (note: NoteWritten) => void;
    renamed?: (from: string, to: string) => void;
    conflict?: (copy: string) => void;
    gone?: (path: string) => void;
    /** Text typed into a loose file is kept apart from it (`kept`), or nothing is kept for it any more. */
    pending?: (path: string, kept: boolean) => void;
    /** A save of a loose file found it is not the version the person started from (`diskHash`; null when it is gone). */
    looseConflict?: (path: string, diskHash: string | null) => void;
  } = {};
  /** Whether a path is a plain text file the window edits (in Source only); the Notes window says which are. */
  textFiles: (path: string) => boolean = () => false;

  active = $derived(this.path !== null);

  private base: Base | null = null;
  /** The file changed during this session (a save or a reload), so the reading view has to catch up on leaving. */
  private touched = false;
  private noteTimer: ReturnType<typeof setTimeout> | null = null;
  private diskTimer: ReturnType<typeof setTimeout> | null = null;
  /** Bumped when a switch starts and when the session ends, so a switch that was waiting for the blocks of a text can tell it was overtaken. */
  private switchToken = 0;
  private checking = false;
  private recheck = false;
  /** Bumped when a save starts and again when it ends, so a disk check can tell it read the file around a save. */
  private saveGeneration = 0;
  private stopCloseGuard: (() => void) | null = null;
  private closeGuardToken = 0;
  private hintSeen = readHintSeen();
  private slowTimer: ReturnType<typeof setTimeout> | null = null;
  /** How many times closing the window was refused because the note could not be saved (the bar that says why is seen first). */
  private closeRefusals = 0;
  /**
   * The text kept apart from the open loose file, which is never written until the person saves; null when none is kept. A
   * loose file is edited with explicit saves (Ctrl+S) and what is typed in between is kept here, so a restart finds it.
   */
  private pendingText: string | null = null;
  /** What failed last for a loose file: writing it, or keeping the text typed into it (Retry does the same again). */
  private failedAt: "save" | "pending" = "save";
  /** "Keep mine" was chosen over a version on disk that was not seen: the next save asks once more before writing it. */
  private overwriteAsk = false;
  /** Saves the open note a moment after the last edit (see `autosave.ts`); idle outside the Notes window. */
  private readonly saver = new Autosaver({ save: () => this.autosaveNow() });

  constructor() {
    docs.guard = (resume) => this.holdNavigation(resume);
  }

  /** The editor that Ctrl+E and E open from reading: the one used last, else the "Open files for editing in" setting. */
  get lastEditMode(): EditMode {
    return session.lastEditMode ?? session.editMode;
  }

  private remember(mode: EditMode): void {
    session.lastEditMode = mode;
  }

  /** Why the open document cannot be edited (null when it can). */
  get blocked(): string | null {
    if (this.readOnlyReason) return this.readOnlyReason;
    const reason = editBlock(docs.current, this.takesText(docs.current?.path));
    return reason ? t(EDIT_BLOCK_TEXT[reason]) : null;
  }

  /** Whether `path` is a plain text file that this window edits. */
  private takesText(path: string | null | undefined): boolean {
    return typeof path === "string" && this.textFiles(path);
  }

  /** The open document is a plain text file: it is edited in Source, and Visual is for Markdown. */
  get sourceOnly(): boolean {
    return docs.current?.kind === "text" && this.takesText(docs.current.path);
  }

  /** The open file is a loose file of the Notes window, edited with explicit saves. */
  get loose(): boolean {
    return this.autosave && isLoosePath(this.path);
  }

  /** The hash of the version of the file the editor started from (null when nothing is being edited). */
  get diskHash(): string | null {
    return this.base?.hash ?? null;
  }

  // ---- entering and leaving -------------------------------------------------------------------

  /**
   * Starts editing the open document, in `mode` (the editor used last when not given), with the caret on the block at the top of
   * the viewport and that block kept at the top. A document that was just made (`fresh`) opens with the caret after its title.
   * `at` starts it at a given place instead (a note comes back where it was left).
   */
  async enter(
    fresh = false,
    mode?: EditMode,
    at?: EnterAt,
    kept?: PendingEdits | null,
  ): Promise<boolean> {
    if (this.active) return true;
    const path = docs.path;
    if (this.loading || !path || !docs.current || docs.deleted) return false;
    if (this.readOnlyReason) {
      toast.message(this.readOnlyReason, "warn");
      return false;
    }
    const blocked = editBlock(docs.current, this.takesText(path));
    if (blocked) {
      toast.message(t(EDIT_BLOCK_TEXT[blocked]), "warn");
      return false;
    }
    this.loading = true;
    try {
      const source = await api.documentSource(path);
      if (docs.path !== path || this.active) return false;
      const line = at ? at.caret : fresh ? 0 : Math.max(0, (bridge.topLine?.() ?? 1) - 1);
      // a plain text file is edited in Source, whatever editor was used last
      const editor = this.takesText(path) ? "source" : (mode ?? this.lastEditMode);
      // the text typed into a loose file and not saved comes back: the editor starts from it, over the version of the file
      let working: { text: string; parsed: ParsedBlocks | null } | undefined;
      if (kept && kept.text !== source.text) {
        working = {
          text: kept.text,
          parsed: editor === "visual" ? await api.parseBlocks(kept.text) : null,
        };
        if (docs.path !== path || this.active) return false;
      }
      this.begin(source, line, fresh, editor, at?.top, working);
      if (kept) this.pendingText = kept.text;
      return true;
    } catch (error) {
      const reason = editBlockFromKind(errorKind(error));
      toast.message(
        reason
          ? t(EDIT_BLOCK_TEXT[reason])
          : t("edit.openFailed", { name: basename(path), message: errorMessage(error) }),
        "warn",
      );
      return false;
    } finally {
      this.loading = false;
    }
  }

  private begin(
    source: SourceDoc,
    line: number,
    atEnd: boolean,
    mode: EditMode,
    top?: number,
    working?: { text: string; parsed: ParsedBlocks | null },
  ): void {
    const { buffer, format } = toBuffer(source.text);
    const map = buildBlockMap(source.text, source);
    this.base = {
      text: source.text,
      buffer,
      format,
      hash: source.hash,
      map,
      modifiedMs: source.modifiedMs,
    };
    this.modifiedMs = source.modifiedMs;
    this.mode = mode;
    this.remember(mode);
    this.switching = false;
    this.switchToken++;
    this.unsavedBlocks = 0;
    this.initial = null;
    this.visualStart = null;
    this.pendingText = null;
    this.failedAt = "save";
    this.overwriteAsk = false;
    if (this.mode === "visual") {
      // the text typed and not saved is the working text: its blocks, and which of them differ from the file
      const typed = working?.parsed ? buildBlockMap(working.text, working.parsed) : null;
      const shown = typed ?? map;
      this.visualStart = {
        map: shown,
        ...(typed
          ? { unsaved: matchBlocks(map, typed).origins.map((origin) => origin === null) }
          : {}),
        block: Math.max(0, blockAtLine(shown, line)),
        atEnd,
        dirty: working !== undefined,
        reveal: "start",
        ...(top === undefined ? {} : { top: Math.max(0, blockAtLine(shown, top)) }),
      };
    } else {
      const start = working ? toBuffer(working.text).buffer : buffer;
      this.initial = { buffer: start, line, atEnd, top: top ?? line };
    }
    this.caret = { line: line + 1, column: 1 };
    this.dirty = working !== undefined;
    this.saving = false;
    this.saveError = null;
    this.conflict = null;
    this.dropPrompt();
    this.note = null;
    this.touched = false;
    this.path = source.path;
    docs.editing = true;
  }

  /** Ctrl+E: reading to the editor used last, and any editor back to reading. */
  toggle(): void {
    if (this.active) this.requestExit();
    else void this.enter();
  }

  /** Done, Read, Ctrl+E or Esc: back to reading, after asking when there is unsaved work. */
  requestExit(): void {
    if (!this.active) return;
    if (this.autosave && !(this.loose && this.dirty)) {
      // a note is saved, not asked about (a loose file with text that was not saved asks, as a document does)
      void this.flush().then((saved) => {
        if (saved) return this.leave({ refresh: true });
      });
      return;
    }
    if (this.dirty) {
      this.ask({ reason: "exit", resume: null });
      return;
    }
    void this.leave({ refresh: true });
  }

  /** Leaves editing without asking (the caller has dealt with unsaved work). */
  async leave(opts: { refresh: boolean }): Promise<void> {
    if (!this.active) return;
    // the reading view comes back with the block that was at the top of the editor at the top
    const line = bridge.editor?.topLine() ?? this.caret.line - 1;
    // the file is read before the reading view comes back, so it never shows the old text for a moment
    const fresh = opts.refresh && this.touched ? await docs.fetchCurrent() : null;
    if (!this.active) return;
    this.clear();
    if (!opts.refresh) return;
    if (fresh) docs.applyQuiet(fresh);
    await tick();
    docs.jumpTo(null, { line: line + 1, find: null, tint: false, align: "top" });
  }

  /** Forgets everything about the session (another project was opened). */
  reset(): void {
    this.clear();
    this.hint = false;
    session.lastEditMode = null;
  }

  private clear(): void {
    this.cancelTimers();
    this.path = null;
    this.base = null;
    this.initial = null;
    this.visualStart = null;
    this.switching = false;
    this.switchToken++;
    this.unsavedBlocks = 0;
    this.saveError = null;
    this.conflict = null;
    this.dropPrompt();
    this.note = null;
    this.saving = false;
    this.touched = false;
    this.pendingText = null;
    this.closeRefusals = 0;
    this.overwriteAsk = false;
    this.setDirty(false);
    docs.editing = false;
  }

  private cancelTimers(): void {
    if (this.noteTimer !== null) clearTimeout(this.noteTimer);
    if (this.diskTimer !== null) clearTimeout(this.diskTimer);
    if (this.slowTimer !== null) clearTimeout(this.slowTimer);
    this.noteTimer = null;
    this.diskTimer = null;
    this.slowTimer = null;
    this.savingSlow = false;
    this.saver.cancel();
  }

  // ---- unsaved work -----------------------------------------------------------------------------

  /** The Source editor reports every change with the length of its text. */
  noteChange(length: number): void {
    const base = this.base;
    if (!base) return;
    if (length !== base.buffer.length) this.setDirty(true);
    else this.setDirty(this.sourceBuffer() !== base.buffer);
    this.touchAutosave();
  }

  /** The Visual editor reports every change of the document or the selection. */
  noteVisual(): void {
    const editor = bridge.editor;
    if (!this.base || editor?.mode !== "visual") return;
    const status = editor.status();
    this.unsavedBlocks = status.changed;
    this.setDirty(status.dirty);
    this.touchAutosave();
  }

  /** An edit was made in a note: a save is due soon. */
  private touchAutosave(): void {
    if (!this.autosave) return;
    // a loose file keeps its text apart; once it is the file's own text again, what was kept goes
    if (this.dirty || (this.loose && this.pendingText !== null)) this.saver.touch();
  }

  /** The text of the Source editor's buffer (no BOM, LF line endings), or the saved one when there is no Source editor. */
  private sourceBuffer(): string {
    const editor = bridge.editor;
    return editor?.mode === "source" ? editor.getText() : (this.base?.buffer ?? "");
  }

  noteCaret(line: number, column: number): void {
    this.caret = { line, column };
  }

  private setDirty(dirty: boolean): void {
    if (this.dirty === dirty) return;
    this.dirty = dirty;
    // the Notes window closes without asking (the notes flush first) and has no unsaved mark in its title
    if (this.autosave) return;
    const title = dirty
      ? t("edit.windowTitle", { name: basename(this.path ?? "") })
      : t("app.name");
    void setWindowTitle(title);
    const token = ++this.closeGuardToken;
    this.stopCloseGuard?.();
    this.stopCloseGuard = null;
    if (!dirty) return;
    // closing the window while there is unsaved work asks first
    void guardWindowClose(() => this.confirmLeave("close")).then((stop) => {
      if (token === this.closeGuardToken) this.stopCloseGuard = stop;
      else stop();
    });
  }

  /**
   * The window is about to close, or the app to restart to update (neither goes through the bar): may it? Asks with
   * the same three choices as the bar. Resolves to false when the person keeps editing or the save did not work.
   */
  async confirmLeave(reason: "close" | "restart"): Promise<boolean> {
    // a note is saved first, and that is all: closing never asks. A note that cannot be saved keeps the window open, so the bar
    // that says why (with "Copy all") is seen. The next try writes it again (the cause may be gone) and, if it still cannot,
    // keeps the text as a note of its own; only when not even that works the window is kept open once more, then it closes.
    if (this.autosave) {
      if (await this.flush()) return true;
      if (reason === "restart") return false;
      if (this.closeRefusals === 0) {
        this.closeRefusals = 1;
        return false;
      }
      if (await this.keepAsNewNote()) return true;
      if (this.closeRefusals === 1) {
        this.closeRefusals = 2;
        return false;
      }
      return true;
    }
    if (!this.dirty) return true;
    const message = reason === "close" ? "edit.closePrompt" : "edit.restartPrompt";
    const choice = await askUnsaved(t(message, { name: basename(this.path ?? "") }), {
      save: t("edit.save"),
      discard: t("edit.discard"),
      keep: t("edit.keepEditing"),
    });
    if (choice === "keep") return false;
    if (choice === "discard") return true;
    return (await this.save()) && !this.dirty;
  }

  /**
   * Called before the open document is left (a link, the tree, Back, another project). While editing with unsaved
   * work the move waits for the person's answer; with a clean buffer editing just ends and the move goes on.
   */
  holdNavigation(resume: () => void): boolean {
    if (!this.active) return false;
    if (this.autosave) {
      if (!this.dirty && !this.saver.busy && this.pendingText === null) {
        void this.leave({ refresh: false });
        return false;
      }
      // a note is saved first; the move goes on once it is (a note that cannot be saved is not left)
      void this.flush().then(async (saved) => {
        if (!saved) return;
        await this.leave({ refresh: false });
        resume();
      });
      return true;
    }
    if (!this.dirty) {
      void this.leave({ refresh: false });
      return false;
    }
    this.ask({ reason: "navigate", resume });
    return true;
  }

  private ask(prompt: UnsavedPrompt): void {
    const first = this.prompt === null;
    this.prompt = prompt;
    if (first) {
      // Esc is "Keep editing"
      this.forgetPrompt = registerOverlay(() => this.keepEditing());
    }
  }

  /** Takes the bar's entry out of the Esc stack; set only while the bar is up. */
  private forgetPrompt: (() => void) | null = null;

  /**
   * The one way the bar goes away: every path that clears `prompt` (an answer, a clean leave, a new session) comes through here, so
   * its entry in the overlay stack is released with it. A leftover entry would make Follow and the single-letter shortcuts think a
   * dialog is open for good.
   */
  private dropPrompt(): UnsavedPrompt | null {
    const prompt = this.prompt;
    this.prompt = null;
    this.forgetPrompt?.();
    this.forgetPrompt = null;
    return prompt;
  }

  /** "Save" on the bar. */
  async promptSave(): Promise<void> {
    if (!this.prompt) return;
    if (!(await this.save())) return;
    this.finishPrompt();
  }

  /** "Discard" on the bar. */
  promptDiscard(): void {
    // the text typed into a loose file was kept apart: discarding it means it is not kept either
    const path = this.path;
    if (this.loose && path && this.pendingText !== null) {
      this.pendingText = null;
      void api
        .clearPending(path)
        .then(() => this.noteHooks.pending?.(path, false))
        .catch(() => {});
    }
    this.finishPrompt();
  }

  private finishPrompt(): void {
    const prompt = this.dropPrompt();
    if (!prompt) return;
    void this.leave({ refresh: prompt.reason === "exit" }).then(() => prompt.resume?.());
  }

  /** "Keep editing" on the bar (and Esc). */
  keepEditing(): void {
    if (!this.prompt) return;
    this.dropPrompt();
    bridge.editor?.focus();
  }

  // ---- saving -------------------------------------------------------------------------------------

  private currentText(): string {
    const base = this.base;
    const editor = bridge.editor;
    // Visual writes the file text itself; the Source buffer gets the file's BOM and line endings back
    if (editor?.mode === "visual") return editor.text();
    if (editor?.mode === "source") {
      const buffer = editor.getText();
      return base ? fromBuffer(buffer, base.format, base) : buffer;
    }
    // No editor is on screen yet (its code is loading after a switch): the text it is about to open with is the working text, and the
    // saved one may be older
    if (this.visualStart) return joinBlockMap(this.visualStart.map);
    if (this.initial && base) return fromBuffer(this.initial.buffer, base.format, base);
    return base?.text ?? "";
  }

  /** Ctrl+S and the Save button. Resolves to true when the file now holds the buffer. */
  async save(): Promise<boolean> {
    const base = this.base;
    const path = this.path;
    if (!base || !path || this.saving) return false;
    if (this.conflict) {
      // writing now would overwrite the other version unseen: the bar asks first
      this.conflictFocus++;
      return false;
    }
    const text = this.currentText();
    // a note is saved as it is typed; a loose file is written only here, when the person asks
    const loose = this.loose;
    if (this.autosave && !loose) return this.saveNote(base, path, text);
    if (loose && this.overwriteAsk) {
      // the person kept their text over a version of the file they never saw: once more before it is replaced
      if (
        !(await askYesNo(
          t("notes.file.overwrite"),
          t("notes.file.overwriteYes"),
          t("common.cancel"),
        ))
      ) {
        return false;
      }
      if (this.path !== path) return false;
      this.overwriteAsk = false;
    }
    this.beginSave();
    try {
      const outcome = await api.saveDocument(path, text, base.hash);
      if (this.path !== path) return false;
      if (outcome.result === "conflict") {
        // not the version the person started from: nothing was written; the Notes window asks what to do
        if (loose) this.noteHooks.looseConflict?.(path, outcome.diskHash);
        else this.setConflict(outcome.diskHash);
        return false;
      }
      await this.adopt(text, outcome.hash, outcome.modifiedMs);
      if (loose) {
        // the file holds the text now, and what was kept apart for it is gone
        this.pendingText = null;
        this.noteHooks.pending?.(path, false);
        this.noteHooks.written?.({ path, created: false, text });
      }
      return true;
    } catch (error) {
      if (this.path === path) {
        this.saveError = errorMessage(error);
        this.failedAt = "save";
      }
      return false;
    } finally {
      this.endSave();
    }
  }

  private beginSave(): void {
    this.saving = true;
    this.saveGeneration++;
    this.saveError = null;
    if (this.autosave) {
      if (this.slowTimer !== null) clearTimeout(this.slowTimer);
      this.slowTimer = setTimeout(() => {
        this.slowTimer = null;
        if (this.saving) this.savingSlow = true;
      }, SLOW_SAVE_MS);
    }
  }

  private endSave(): void {
    this.saving = false;
    this.saveGeneration++;
    if (this.slowTimer !== null) clearTimeout(this.slowTimer);
    this.slowTimer = null;
    this.savingSlow = false;
    this.runRecheck();
  }

  /**
   * Saves the text of a note (autosave, a flush, Ctrl+S). The note is written unless it changed since the editor started from it:
   * then nothing of the other version is overwritten, the text the person has is kept as a copy of its own, and the editor shows
   * the version on disk. A new note has no file until its text has something in it; its first save may give it another name.
   */
  private async saveNote(base: Base, path: string, text: string): Promise<boolean> {
    this.beginSave();
    try {
      let outcome = await api.saveNote(path, text, base.hash);
      if (this.path !== path) return false;
      for (let again = 0; outcome.result === "conflict" && again < 3; again++) {
        this.noteHooks.conflict?.(outcome.copy);
        // typed while the save ran: that goes to a copy as well, so the editor can take the version on disk
        const typed = this.currentText();
        if (typed === text) break;
        text = typed;
        outcome = await api.saveNote(path, text, base.hash);
        if (this.path !== path) return false;
      }
      if (outcome.result === "blank") return true;
      if (outcome.result === "conflict") {
        await this.reloadTheirs(true);
        return false;
      }
      if (outcome.path !== path) {
        this.path = outcome.path;
        this.noteHooks.renamed?.(path, outcome.path);
      }
      await this.adopt(text, outcome.hash, outcome.modifiedMs);
      this.closeRefusals = 0;
      this.noteHooks.written?.({ path: this.path ?? outcome.path, created: outcome.created, text });
      return true;
    } catch (error) {
      if (this.path === path) this.saveError = errorMessage(error);
      return false;
    } finally {
      this.endSave();
    }
  }

  /**
   * Saves what is waiting now, and resolves to whether the open note is saved (always true when nothing is open). A save that
   * failed earlier left the text unsaved with nothing to try again, so it is tried again here: the cause may be gone.
   */
  async flush(): Promise<boolean> {
    if (!this.autosave || !this.active) return true;
    try {
      await this.saver.flush();
    } catch {
      // the failure is on the bar already
    }
    if (this.loose) {
      // a loose file is not written: its text is kept apart, and that is all there is to do before it is left
      await this.keepPending();
      return this.saveError === null || this.failedAt === "save";
    }
    if (this.dirty && !this.saving && this.conflict === null) await this.save();
    return !this.dirty && this.saveError === null;
  }

  /** The text of the open note, written as a new note: what is left when the note itself cannot be written. */
  private async keepAsNewNote(): Promise<boolean> {
    try {
      const outcome = await api.saveNote(await api.newNote(), this.currentText(), "");
      return outcome.result === "saved";
    } catch {
      return false;
    }
  }

  /** The autosave timer fired: save the note, or keep the text typed into a loose file, if either needs it. */
  private async autosaveNow(): Promise<void> {
    if (!this.path) return;
    if (this.loose) {
      await this.keepPending();
      return;
    }
    if (!this.dirty) return;
    await this.save();
  }

  /**
   * Keeps the text typed into the open loose file apart from it (never in it), with the version of the file it was typed over; and
   * forgets what was kept once the text is the file's own again. A restart finds the text and the "unsaved" mark.
   */
  private async keepPending(): Promise<void> {
    const path = this.path;
    const base = this.base;
    if (!path || !base) return;
    if (!this.dirty) {
      if (this.pendingText === null) return;
      this.pendingText = null;
      try {
        await api.clearPending(path);
        this.noteHooks.pending?.(path, false);
      } catch {
        // what stays kept is shown again when the file is opened; it is the file's own text then
      }
      return;
    }
    const text = this.currentText();
    if (text === this.pendingText) return;
    try {
      await api.savePending(path, text, base.hash);
      if (this.path !== path) return;
      this.pendingText = text;
      if (this.failedAt === "pending") this.saveError = null;
      this.noteHooks.pending?.(path, true);
    } catch (error) {
      if (this.path === path) {
        this.saveError = errorMessage(error);
        this.failedAt = "pending";
      }
    }
  }

  /** Ends editing at once, without saving what was typed since the last save (the note was deleted, or is no longer wanted). */
  abandon(): void {
    this.clear();
  }

  /** The text the document holds as the editor has it now, as a file would (the note's text for "Copy"). */
  exactText(): string {
    return this.currentText();
  }

  /** A disk check that had to wait (for another check or a save) runs now. */
  private runRecheck(): void {
    if (!this.recheck || this.checking || this.saving) return;
    this.recheck = false;
    void this.checkDisk();
  }

  /** The file now holds `text`: it becomes the version the editor goes on from. */
  private async adopt(text: string, hash: string, modifiedMs: number): Promise<void> {
    const before = this.base;
    if (!before) return;
    // The block map helps (the count of changed blocks) and a Visual document that starts over from the saved text needs it: a failure
    // to read it never fails a save, and is reported.
    let parsed: ParsedBlocks | null = null;
    let unreadable: unknown = null;
    try {
      parsed = await api.parseBlocks(text);
    } catch (error) {
      unreadable = error;
    }
    const map = buildBlockMap(text, parsed ?? { eol: "lf", blocks: [] });
    const changed = changedBlockCount(matchBlocks(before.map, map));
    const { buffer } = toBuffer(text);
    this.base = { ...before, text, buffer, hash, map, modifiedMs };
    this.modifiedMs = modifiedMs;
    this.touched = true;
    // without blocks to go by, the editor keeps the ones it has (they give the same text), unless there is nothing in the text to lose
    this.followSave(text, parsed !== null || text.trim() === "" ? map : null, bridge.editor);
    // a note says "Saved" in the status bar and nothing in the header; a loose file, written only when asked, says so for a moment
    if (!this.autosave || this.loose)
      this.showNote(changed > 0 ? tn("edit.savedBlocks", changed) : t("edit.saved"));
    if (unreadable !== null) {
      toast.message(t("edit.blocksUnreadable", { message: errorMessage(unreadable) }), "warn");
    }
    if (!this.hintSeen && !this.autosave) {
      this.hintSeen = true;
      this.hint = true;
      try {
        localStorage.setItem(HINT_KEY, "1");
      } catch {
        // the hint simply shows again next launch
      }
    }
  }

  /**
   * The saved text is the version the editor goes on from. Source compares its buffer with it again. Visual starts over from the
   * saved blocks, so the unsaved-block bar clears and the next save compares with what was just written; unless the person typed
   * while the file was being written, then the document is left as it is (nothing is lost, the bar stays a little longer). Without
   * `map` (the blocks of the saved text could not be read) it is left as it is too.
   */
  private followSave(text: string, map: BlockMap | null, editor: EditorApi | undefined): void {
    if (editor?.mode === "visual") {
      if (map && editor.text() === text) editor.rebase(map);
      this.noteVisual();
    } else if (editor?.mode === "source") this.noteChange(editor.getText().length);
    else {
      // No editor is mounted yet: it will open on the version that was just saved.
      if (map && this.mode === "visual" && this.visualStart) {
        this.visualStart = { ...this.visualStart, map, unsaved: undefined, dirty: false };
      }
      this.setDirty(false);
    }
  }

  private showNote(text: string): void {
    this.note = text;
    if (this.noteTimer !== null) clearTimeout(this.noteTimer);
    this.noteTimer = setTimeout(() => {
      this.note = null;
    }, NOTE_MS);
  }

  dismissHint(): void {
    this.hint = false;
  }

  /** "Retry" on the save error bar. */
  retry(): void {
    if (this.loose && this.failedAt === "pending") void this.keepPending();
    else void this.save();
  }

  /** "Copy all" on the save error bar: the whole text, so nothing is lost if the file cannot be written. */
  async copyAll(): Promise<boolean> {
    return copyText(this.currentText());
  }

  dismissSaveError(): void {
    this.saveError = null;
  }

  // ---- the file changed on disk ---------------------------------------------------------------------

  /** Every `fs-change` batch passes here; the ones that touch the file being edited are looked at. */
  onFsChange(batch: FsBatch): void {
    const path = this.path;
    if (!path || !this.base) return;
    const mine = batch.changes.some((c) => c.path === path || c.from === path);
    if (!mine && batch.bulk === null && !batch.rescan) return;
    this.scheduleDiskCheck();
  }

  private scheduleDiskCheck(): void {
    if (this.diskTimer !== null) clearTimeout(this.diskTimer);
    this.diskTimer = setTimeout(() => {
      this.diskTimer = null;
      void this.checkDisk();
    }, DISK_CHECK_DELAY_MS);
  }

  // ---- file operations of the Explorer --------------------------------------------------------------

  /**
   * The document being edited and the text in the editor, when it has unsaved edits. A move does not write such a
   * document: it hands back the changes for this text instead (see [`EditStore.followMoves`]).
   */
  unsavedBuffer(): BufferText | null {
    const editor = bridge.editor;
    return this.path !== null && this.dirty && editor?.mode === "source"
      ? { path: this.path, text: editor.getText() }
      : null;
  }

  /**
   * The document being edited when it has unsaved edits that cannot be patched with link changes: the ones in Visual
   * mode (its document is blocks, not text to splice into), or whose editor is not on screen yet. A move leaves such a
   * document alone and says so. The Source editor takes the changes instead (see [`EditStore.unsavedBuffer`]).
   */
  unsavedUnpatchable(): string | null {
    return this.path !== null && this.dirty && bridge.editor?.mode !== "source" ? this.path : null;
  }

  /**
   * Files or folders were moved or renamed in the Explorer (the file system does not report it: the watcher is told it
   * is our own doing). The document being edited keeps its text and the version of the file it started from, under its
   * new path. `changes` are the link updates for the unsaved text `sent`; they are applied to the editor only if the
   * text is still exactly that. The file is looked at again, because its links may have been rewritten on disk (a
   * buffer with no edits takes them in). Resolves to false when the changes could not be applied.
   */
  followMoves(moves: readonly Move[], changes?: BufferEdits, sent?: string): boolean {
    if (!this.path) return true;
    this.path = mapPath(this.path, moves);
    let applied = true;
    if (changes && changes.edits.length > 0) {
      const editor = bridge.editor;
      if (editor?.mode === "source" && editor.getText() === sent)
        editor.applyChanges(changes.edits);
      else applied = false;
    }
    this.scheduleDiskCheck();
    return applied;
  }

  /** Files or folders were deleted in the Explorer: if the document being edited was one of them, the bar says so. */
  followRemoved(): void {
    if (this.path) this.scheduleDiskCheck();
  }

  /** Compares the file with the version being edited: clean buffers reload, dirty ones get the banner. */
  private async checkDisk(): Promise<void> {
    // a loose file is looked at by the Notes window, which never replaces what is shown on its own
    if (this.loose) return;
    // while a save runs the file is about to change under the check; it runs again when the save is over
    if (this.checking || this.saving) {
      this.recheck = true;
      return;
    }
    const path = this.path;
    if (!path || !this.base) return;
    const generation = this.saveGeneration;
    this.checking = true;
    try {
      const source = await api.documentSource(path);
      if (this.path !== path || !this.base) return;
      if (generation !== this.saveGeneration) {
        // a save started or ended while the file was being read: this may be the version from before it
        this.recheck = true;
        return;
      }
      if (source.hash === this.base.hash) {
        // our own save, or a change that was undone: the file is the version being edited again
        this.conflict = null;
        return;
      }
      if (this.dirty) {
        // a note is never left waiting for an answer: the save keeps the text as a copy and the editor shows the version on disk
        if (this.autosave) void this.save();
        else this.setConflict(source.hash);
      } else this.applyDisk(source, !this.autosave);
    } catch (error) {
      if (this.path !== path) return;
      if (generation !== this.saveGeneration) this.recheck = true;
      else if (errorKind(error) === "notFound") this.fileGone(path);
    } finally {
      this.checking = false;
      this.runRecheck();
    }
  }

  /**
   * The file being edited is gone. A document asks what to do (the bar); a note that has unsaved text is written again, and one that
   * has none is simply no longer there.
   */
  private fileGone(path: string): void {
    if (!this.autosave) {
      this.setConflict(null);
      return;
    }
    if (this.dirty) void this.save();
    else this.noteHooks.gone?.(path);
  }

  private setConflict(diskHash: string | null): void {
    this.conflict = { atMs: Date.now(), deleted: diskHash === null, diskHash, confirmReload: null };
  }

  /** Puts the file's version into the editor, keeping the caret on the same block when it can. */
  private applyDisk(source: SourceDoc, announce: boolean): void {
    const before = this.base;
    if (!before) return;
    const { buffer, format } = toBuffer(source.text);
    const map = buildBlockMap(source.text, source);
    const match = matchBlocks(before.map, map);
    const editor = bridge.editor;
    // the caret's line in the version being edited (a Visual document that is clean is the saved version)
    const caretLine =
      editor?.mode === "visual"
        ? (before.map.blocks[editor.caretBlock() ?? -1]?.startLine ?? 0)
        : (editor?.caretLine() ?? this.caret.line - 1);
    const line = mapLine(before.map, map, match, caretLine);
    this.base = {
      text: source.text,
      buffer,
      format,
      hash: source.hash,
      map,
      modifiedMs: source.modifiedMs,
    };
    this.modifiedMs = source.modifiedMs;
    this.conflict = null;
    this.saveError = null;
    this.touched = true;
    // the file's own text replaces what was typed: nothing is kept apart for it any more
    if (this.loose && this.pendingText !== null) {
      this.pendingText = null;
      this.overwriteAsk = false;
      const path = this.path;
      if (path) this.noteHooks.pending?.(path, false);
    }
    if (this.mode === "visual") {
      const input: VisualStart = { map, block: Math.max(0, blockAtLine(map, line)), dirty: false };
      if (editor?.mode === "visual") editor.load(input);
      else this.visualStart = input;
      this.unsavedBlocks = 0;
    } else if (editor?.mode === "source") editor.setText(buffer, line);
    else this.initial = { buffer, line, atEnd: false };
    this.setDirty(false);
    if (announce) this.showNote(t("edit.reloaded", { time: formatClock(Date.now()) }));
  }

  /** "Reload theirs": the editor takes the file's version and the unsaved edits are dropped. */
  async reloadTheirs(confirmed = false): Promise<void> {
    const path = this.path;
    const base = this.base;
    if (!path || !base) return;
    try {
      if (!confirmed && this.dirty) {
        const pending = await this.dirtyBlocks();
        if (pending > CONFIRM_RELOAD_BLOCKS && this.conflict) {
          this.conflict = { ...this.conflict, confirmReload: pending };
          return;
        }
      }
      const source = await api.documentSource(path);
      if (this.path === path) this.applyDisk(source, !this.autosave);
    } catch (error) {
      if (errorKind(error) === "notFound") this.fileGone(path);
      else this.saveError = errorMessage(error);
    }
  }

  /** How many blocks the unsaved edits touch. */
  private async dirtyBlocks(): Promise<number> {
    const base = this.base;
    if (!base) return 0;
    const text = this.currentText();
    const parsed = await api.parseBlocks(text).catch(() => ({ eol: "lf" as const, blocks: [] }));
    return changedBlockCount(matchBlocks(base.map, buildBlockMap(text, parsed)));
  }

  cancelReload(): void {
    if (this.conflict) this.conflict = { ...this.conflict, confirmReload: null };
  }

  /**
   * "Keep mine" for a loose file whose version on disk was not seen: the text stays as it is and is not written now. The editor
   * goes on from the version on disk, so the next save passes the check, but asks once more before it replaces that version.
   */
  keepMineOver(diskHash: string | null): void {
    const base = this.base;
    if (!base || diskHash === null) return;
    this.base = { ...base, hash: diskHash };
    this.overwriteAsk = true;
  }

  /** "Keep mine": the buffer is written over the version on disk. */
  async keepMine(): Promise<void> {
    const conflict = this.conflict;
    const base = this.base;
    if (!conflict || !base || conflict.deleted || conflict.diskHash === null) return;
    // the version on disk is the one this save is based on, so the check passes
    this.base = { ...base, hash: conflict.diskHash };
    this.conflict = null;
    await this.save();
  }

  /** "Save as new file": the file was deleted, so the buffer is written again under its name. */
  async saveAsNew(): Promise<void> {
    const path = this.path;
    const base = this.base;
    if (!path || !base || this.saving) return;
    const text = this.currentText();
    this.beginSave();
    try {
      const created = await api.createDocument(path, text);
      if (this.path !== path) return;
      await this.adopt(text, created.hash, created.modifiedMs);
      this.conflict = null;
      if (this.loose) {
        // a loose file written again where it was: nothing is kept apart for it any more
        this.pendingText = null;
        this.noteHooks.pending?.(path, false);
      }
      void vault.refreshTree();
    } catch (error) {
      if (this.path === path) this.saveError = errorMessage(error);
    } finally {
      this.endSave();
    }
  }

  /** "Discard" on the deleted-on-disk bar: the edits are dropped and the document is shown as deleted. */
  discardDeleted(): void {
    this.clear();
    docs.noteDeleted();
  }

  // ---- the modes: Read, Visual and Source -----------------------------------------------------------------

  /**
   * A segment of the header switch: one click goes to any mode. Read asks first when there is unsaved work (the bar), Visual and
   * Source keep it when going from one to the other; from reading, an editor opens at the block at the top of the viewport.
   */
  async goTo(target: DocMode): Promise<void> {
    if (target === "read") this.requestExit();
    else if (this.active) await this.switchTo(target);
    else await this.enter(false, target);
  }

  /** Ctrl+Alt+E: between the two editors; from reading, the one that was not used last. */
  async toggleMode(): Promise<void> {
    if (this.active) await this.switchTo(otherEditMode(this.mode));
    else await this.enter(false, otherEditMode(this.lastEditMode));
  }

  /**
   * Opens the other editor on the text as it is now, unsaved work included, with the caret in the same block and the block that
   * was at the top of the view still there. `block` (of the text being edited) puts the caret, and the view, on a given block
   * instead: the "Edit in Source" button of a locked block. Resolves to whether the editor changed. Source to Visual waits for
   * the blocks of the text, so a second switch meanwhile is ignored, and so is a switch that the end of the session or another
   * switch overtook.
   */
  async switchTo(mode: EditMode, opts: { block?: number | null } = {}): Promise<boolean> {
    const base = this.base;
    const editor = bridge.editor;
    if (!this.active || !base || !editor || this.switching || mode === this.mode) return false;
    // a plain text file has no blocks to show: Visual is for Markdown
    if (mode === "visual" && this.sourceOnly) {
      toast.message(t("edit.visualForMarkdown"), "info");
      return false;
    }
    const token = ++this.switchToken;
    this.switching = true;
    try {
      if (editor.mode === "visual") {
        // the text a save would write, so Source shows exactly that
        const text = editor.text();
        const line = editor.lineOfBlock(opts.block ?? null);
        const top = opts.block == null ? editor.topLine() : line;
        const { buffer } = toBuffer(text);
        this.initial = { buffer, line, atEnd: false, top };
        this.visualStart = null;
        this.mode = "source";
        this.remember("source");
        this.caret = { line: line + 1, column: 1 };
        this.unsavedBlocks = 0;
        this.setDirty(buffer !== base.buffer);
        return true;
      }
      // Source to Visual: the blocks of the buffer come from the backend
      let text = this.currentText();
      let line = editor.caretLine();
      let top = editor.topLine();
      let parsed = await api.parseBlocks(text);
      // typing while the blocks were being read: read them again for the later text; if it keeps changing, the blocks that were
      // read are of an older text and the switch would drop what was typed since
      for (let rereads = 0; token === this.switchToken && this.currentText() !== text; rereads++) {
        if (rereads === 3) throw new Error(t("edit.switchTyping"));
        text = this.currentText();
        line = editor.caretLine();
        top = editor.topLine();
        parsed = await api.parseBlocks(text);
      }
      if (token !== this.switchToken || !this.active || this.mode !== "source") return false;
      // A save or a reload can land while the blocks are read: what the text is compared with is the version there is now.
      const saved = this.base;
      if (!saved) return false;
      const map = buildBlockMap(text, parsed);
      const match = matchBlocks(saved.map, map);
      this.visualStart = {
        map,
        unsaved: match.origins.map((origin) => origin === null),
        dirty: text !== saved.text,
        block: Math.max(0, blockAtLine(map, line)),
        top: Math.max(0, blockAtLine(map, top)),
      };
      this.initial = null;
      this.mode = "visual";
      this.remember("visual");
      return true;
    } catch (error) {
      toast.message(t("edit.switchFailed", { message: errorMessage(error) }), "warn");
      return false;
    } finally {
      if (token === this.switchToken) this.switching = false;
    }
  }

  /** The Visual editor could not be loaded: Source takes over with the text the editor was to open (unsaved work included), and says so. */
  visualUnavailable(): void {
    const base = this.base;
    if (!this.active || !base || this.mode !== "visual") return;
    // the working text lives in what the Visual editor was to start from, not in the saved file
    const start = this.visualStart;
    const text = start ? joinBlockMap(start.map) : base.text;
    const block = start?.block;
    const line =
      block === undefined || block === null ? 0 : (start?.map.blocks[block]?.startLine ?? 0);
    this.initial = { buffer: toBuffer(text).buffer, line, atEnd: false };
    this.visualStart = null;
    this.mode = "source";
    this.caret = { line: line + 1, column: 1 };
    this.setDirty(text !== base.text);
    toast.message(t("edit.visualFailed"), "warn");
  }

  // ---- images -----------------------------------------------------------------------------------------

  /** Images pasted or dropped into the editor are written next to the document and linked at the caret. */
  async addFiles(files: File[], source: "paste" | "drop"): Promise<void> {
    const path = this.path;
    const editor = bridge.editor;
    if (!path || !editor) return;
    for (const file of files) {
      const extension = imageExtension(file.type) ?? (isImageName(file.name) ? "" : null);
      if (extension === null) {
        toast.message(t("images.onlyImages"), "warn");
        continue;
      }
      const name =
        source === "paste" || file.name === ""
          ? pastedImageName(path, extension || "png", new Date())
          : file.name;
      if (file.size > MAX_IMAGE_BYTES) {
        // refused before the file is read: a huge one would freeze the window and then be refused anyway
        toast.message(
          t("images.tooBig", {
            name: file.name || name,
            size: megabytes(file.size),
            limit: megabytes(MAX_IMAGE_BYTES),
          }),
          "warn",
        );
        continue;
      }
      if (file.size > CONFIRM_ABOVE_BYTES) {
        const size = megabytes(file.size);
        const ok = await askYesNo(
          t("images.tooLarge", { name: file.name || name, size }),
          t("images.add"),
          t("common.cancel"),
        );
        if (!ok) continue;
      }
      try {
        const written = await api.writeAsset(path, name, await toBase64(file));
        if (this.path !== path) return;
        editor.insertImage(written.rel);
      } catch (error) {
        toast.message(t("images.addFailed", { message: errorMessage(error) }), "warn");
      }
    }
    editor.focus();
  }
}

export const edit = new EditStore();
