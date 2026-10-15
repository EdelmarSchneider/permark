// Review notes as the window sees them (docs/design/v0.4.md section 3): the notes the backend keeps, the note being
// written, and what the document view and the Notes tab need to show them. The backend finds each note's passage again
// whenever the document changes; this store asks for the list when something could have changed it.

import { api } from "../api";
import { bridge } from "../bridge";
import { highlight, type NoteTarget } from "../dom/noteDom";
import { errorMessage } from "../errors";
import { t } from "../i18n";
import { doneNotes, openByDocument, openCount } from "../notes";
import { batchTouches } from "../unread";
import type { FsBatch, Note } from "../types";
import { docs } from "./docs.svelte";
import { toast } from "./toast.svelte";
import { vault } from "./vault.svelte";

/** The list is asked for this long after the last thing that could have changed it. */
export const REFRESH_DELAY_MS = 400;
/** A selection seen a moment ago still counts when the field is asked for from somewhere that took the selection away. */
export const REMEMBER_MS = 60_000;
/** The name of the highlight that keeps the passage visible while its note is typed. */
export const DRAFT_HIGHLIGHT = "tm-note-draft";

/** The note being written: the document and the passage. */
export type NoteDraft = { path: string; target: NoteTarget };

type Remembered = { path: string; target: NoteTarget; atMs: number };

export class NotesStore {
  /** Every note of the project, by document and line. */
  all = $state.raw<readonly Note[]>([]);
  groups = $derived(openByDocument(this.all));
  done = $derived(doneNotes(this.all));
  /** How many notes are open. */
  count = $derived(openCount(this.all));
  /** The field is open on this passage. */
  draft = $state.raw<NoteDraft | null>(null);
  saving = $state(false);
  /** A bar in the margin was clicked: the Notes tab shows this note (`n` makes a second click on it count). */
  focus = $state.raw<{ id: string; n: number } | null>(null);

  private requests = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private remembered: Remembered | null = null;
  private focusCount = 0;

  constructor(private readonly now: () => number = Date.now) {}

  /** A project was opened (or closed): forget everything. */
  reset(): void {
    this.requests++;
    this.clearTimer();
    this.all = [];
    this.cancel();
    this.remembered = null;
    this.focus = null;
  }

  // ---- the list ---------------------------------------------------------------------------------------

  /** Asks the backend for the notes. */
  async refresh(): Promise<void> {
    if (vault.isSimple) return;
    const request = ++this.requests;
    try {
      const notes = await api.listNotes();
      // an answer to a question asked before the project changed, or before a newer question, is stale
      if (request === this.requests) this.all = notes;
    } catch {
      // the next change asks again
    }
  }

  /** The backend says the notes changed. */
  onChanged(): void {
    this.clearTimer();
    void this.refresh();
  }

  /** The document on show was drawn again (opened, saved, changed by someone else): its notes may have moved. */
  documentChanged(): void {
    if (this.all.length > 0) this.schedule();
  }

  /** Files changed on disk: the notes on them may have moved. */
  filesChanged(batch: FsBatch): void {
    if (this.all.length === 0) return;
    if (batch.bulk !== null || this.all.some((note) => batchTouches(batch, note.path))) {
      this.schedule();
    }
  }

  private schedule(): void {
    this.clearTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.refresh();
    }, REFRESH_DELAY_MS);
  }

  private clearTimer(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  // ---- writing a note ---------------------------------------------------------------------------------

  /** Takes note of the selection that is on screen, for a field that is asked for after the selection is gone. */
  remember(): void {
    const target = this.liveTarget();
    if (target && docs.path !== null) {
      this.remembered = { path: docs.path, target, atMs: this.now() };
    }
  }

  private liveTarget(): NoteTarget | null {
    return docs.current?.kind === "markdown" && !docs.editing && !vault.isSimple
      ? (bridge.noteTarget?.() ?? null)
      : null;
  }

  /**
   * Opens the field on the selected passage. Asked for from a button, a key or the palette: the selection is the one on
   * screen, or the one seen a moment ago. Says so when there is none.
   */
  begin(): boolean {
    if (vault.isSimple || docs.editing || docs.current?.kind !== "markdown" || docs.path === null) {
      return false;
    }
    const live = this.liveTarget();
    const kept = this.remembered;
    const recent =
      kept && kept.path === docs.path && this.now() - kept.atMs < REMEMBER_MS ? kept.target : null;
    const target = live ?? recent;
    if (!target) {
      toast.message(t("notes.selectFirst"), "info");
      return false;
    }
    this.remembered = null;
    this.draft = { path: docs.path, target };
    highlight(DRAFT_HIGHLIGHT, target.range);
    return true;
  }

  /** The field closes without a note. */
  cancel(): void {
    if (this.draft === null) return;
    this.draft = null;
    highlight(DRAFT_HIGHLIGHT, null);
  }

  /** Saves the note being written. The field stays open, with its words, when that fails. */
  async save(text: string): Promise<boolean> {
    const draft = this.draft;
    const words = text.trim();
    if (!draft || words === "" || this.saving) return false;
    this.saving = true;
    try {
      const { target } = draft;
      await api.addNote({
        path: draft.path,
        quote: target.quote,
        text: words,
        headingPath: target.headingPath,
        line: target.startLine,
        endLine: target.endLine,
      });
    } catch (error) {
      toast.message(t("notes.failed", { message: errorMessage(error) }), "warn");
      return false;
    } finally {
      this.saving = false;
    }
    this.cancel();
    // the passage is not selected any more: it has a bar
    window.getSelection()?.removeAllRanges();
    await this.refresh();
    return true;
  }

  // ---- the list's own actions ------------------------------------------------------------------------------

  /** The person checks a note off, or opens it again. */
  async setDone(id: string, done: boolean): Promise<void> {
    try {
      const note = await api.setNoteDone(id, done);
      this.all = this.all.map((candidate) => (candidate.id === id ? note : candidate));
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      void this.refresh();
    }
  }

  async remove(id: string): Promise<void> {
    try {
      await api.deleteNote(id);
      this.all = this.all.filter((candidate) => candidate.id !== id);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      void this.refresh();
    }
  }

  /** Asks the Notes tab to show a note (it scrolls to it and marks it for a moment). */
  show(id: string): void {
    this.focus = { id, n: ++this.focusCount };
  }
}

export const notes = new NotesStore();
