// The Notes tab's state: the list the backend keeps (the notes folder, pins, marks), the note shown, the dialogs
// (versions, move, delete, compare) and the actions on them. The backend owns the truth; this mirrors it, asks it to
// change, and keeps the editor of the window on the right note.

import {
  api as defaultApi,
  type Api,
  type ApiError,
  type NoteEntry,
  type NoteList,
  type NotesIntent,
  type RecentFolder,
  type Version,
  type WorkspaceView,
} from "../../api";
import { documents as defaultDocuments } from "../../core/doc/document.svelte";
import { confirmLeave } from "../../core/editing/guard";
import { editing as defaultEditing } from "../../core/editing/session.svelte";
import { apiErrorText } from "../../core/errors";
import { t, tn } from "../../core/i18n";
import { PluginClient, pluginClient } from "../../core/plugins/client";
import { windowStore as defaultWindow, type WindowStore } from "../../core/window.svelte";
import {
  arrange,
  drawnOrder,
  dropPinned,
  filterNotes,
  movePinned,
  neighbour,
  pinnedAt,
  shownName,
  type Arranged,
  type Frozen,
} from "./model";

export type ListState =
  | { status: "loading" }
  | { status: "ready"; list: NoteList }
  | { status: "failed"; error: ApiError };

/** The Versions dialog. */
export type VersionsState =
  | { status: "closed" }
  | {
      status: "open";
      path: string;
      items: readonly Version[] | null;
      selected: string | null;
      preview: string | null;
    };

/** The Move to project dialog. */
export type MoveState =
  | { status: "closed" }
  | {
      status: "open";
      path: string;
      folder: string | null;
      name: string;
      recents: readonly RecentFolder[];
      error: string | null;
      busy: boolean;
    };

/** Asking before a deletion that takes pinned notes, or a file that holds typed text. */
export type ConfirmState =
  | { status: "closed" }
  | { status: "deleteNotes"; paths: readonly string[]; pinned: readonly string[] }
  | { status: "removeFile"; path: string };

/** Mine (recovered) against theirs (the file now). */
export type CompareState =
  { status: "closed" } | { status: "open"; path: string; mine: string; theirs: string };

/** The polling of the list while the window is visible. */
export const POLL_MS = 3000;
/** Saves in a burst refresh the list once. */
export const SAVED_REFRESH_MS = 400;

/** Why a name was refused, in words (`detail` is the backend's code). */
function invalidNameText(code: string | null): string {
  switch (code) {
    case "empty":
      return t("notes.rename.empty");
    case "dot":
      return t("notes.rename.dot");
    case "reserved":
      return t("notes.rename.reserved");
    case "tooLong":
      return t("notes.rename.tooLong");
    case "notMarkdown":
      return t("notes.rename.notMarkdown");
    default:
      return t("notes.rename.chars");
  }
}

function errorOf(result: { ok: false; error: ApiError }): string {
  return apiErrorText(result.error);
}

export class NotesStore {
  list = $state.raw<ListState>({ status: "loading" });
  /** The document shown (a note's file name, a loose file's `@file/<id>/<name>`). */
  current = $state<string | null>(null);
  filter = $state("");
  /** The note being edited keeps its place in the list until it is left. */
  frozen = $state.raw<Frozen | null>(null);
  /** The row being renamed. */
  renaming = $state<string | null>(null);
  versions = $state.raw<VersionsState>({ status: "closed" });
  move = $state.raw<MoveState>({ status: "closed" });
  confirm = $state.raw<ConfirmState>({ status: "closed" });
  compare = $state.raw<CompareState>({ status: "closed" });

  readonly #api: Api;
  readonly #client: PluginClient;
  readonly #documents: typeof defaultDocuments;
  readonly #editing: typeof defaultEditing;
  readonly #window: WindowStore;
  #workspace: Extract<WorkspaceView, { kind: "notes" }> | null = null;
  #seq = 0;
  #restored = false;
  #refreshTimer: ReturnType<typeof setTimeout> | null = null;

  entries = $derived(this.list.status === "ready" ? this.list.list.entries : []);
  folder = $derived(this.list.status === "ready" ? this.list.list.folder : null);
  /** The row of the document shown. */
  entry = $derived(this.entries.find((e) => e.path === this.current) ?? null);

  constructor(
    deps: {
      api?: Api;
      client?: PluginClient;
      documents?: typeof defaultDocuments;
      editing?: typeof defaultEditing;
      window?: WindowStore;
    } = {},
  ) {
    this.#api = deps.api ?? defaultApi;
    this.#client = deps.client ?? pluginClient;
    this.#documents = deps.documents ?? defaultDocuments;
    this.#editing = deps.editing ?? defaultEditing;
    this.#window = deps.window ?? defaultWindow;
  }

  /** The list as drawn (filtered, grouped). `now`: the clock. */
  arranged(now = Date.now()): Arranged {
    return arrange(filterNotes(this.entries, this.filter), now, this.frozen);
  }

  /** The documents in the order the list draws them. */
  drawn(now = Date.now()): NoteEntry[] {
    return drawnOrder(this.arranged(now));
  }

  // ---- the window ------------------------------------------------------------------------------------------------

  /** The window shows the Notes: the reading side knows the workspace, the list is read, the editor follows. */
  attach(workspace: Extract<WorkspaceView, { kind: "notes" }>): () => void {
    this.#workspace = workspace;
    this.#documents.setWorkspace(workspace);
    this.#editing.bindJournal((path, baseHash, text) => {
      void this.#client.call("notes.buffer", { path, baseHash, text });
    });
    const offSaved = this.#editing.onSaved(() => this.#refreshSoon());
    const timer = setInterval(() => {
      if (typeof document === "undefined" || document.visibilityState !== "hidden") {
        void this.refresh();
      }
    }, POLL_MS);
    const onFocus = (): void => void this.refresh();
    const onBlur = (): void => this.#editing.dispatch({ e: "autosaveDue" });
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    // a wish from another view (the Welcome of another window, a project tab) while this tab is open already; before
    // the first read, that read takes it
    let offIntent: (() => void) | null = null;
    let gone = false;
    void this.#api
      .onAppPlugin("notes:intent", () => {
        if (this.#restored && this.#workspace !== null) void this.#takeIntent();
      })
      .then((off) => {
        if (gone) off();
        else offIntent = off;
      });
    void this.refresh();
    return () => {
      gone = true;
      offIntent?.();
      clearInterval(timer);
      if (this.#refreshTimer !== null) clearTimeout(this.#refreshTimer);
      this.#refreshTimer = null;
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      offSaved();
      this.#editing.bindJournal(null);
      this.#workspace = null;
      this.#restored = false;
      this.current = null;
      this.frozen = null;
      this.#seq += 1;
    };
  }

  #fail(error: ApiError): void {
    this.#window.notifyFileOpFailed(error);
  }

  #refreshSoon(): void {
    if (this.#refreshTimer !== null) clearTimeout(this.#refreshTimer);
    this.#refreshTimer = setTimeout(() => {
      this.#refreshTimer = null;
      void this.refresh();
    }, SAVED_REFRESH_MS);
  }

  /** Reads the list again. The newest answer wins; a failure keeps what is shown. */
  async refresh(): Promise<void> {
    const seq = ++this.#seq;
    const result = await this.#client.call("notes.list", {});
    if (seq !== this.#seq || this.#workspace === null) return;
    if (!result.ok) {
      if (this.list.status !== "ready") this.list = { status: "failed", error: result.error };
      return;
    }
    this.list = { status: "ready", list: result.value };
    if (!this.#restored) {
      this.#restored = true;
      // a note (or a new one) was asked for; else the note that was open last time
      const asked = await this.#takeIntent();
      if (asked === null && this.#workspace !== null && result.value.open !== null) {
        await this.select(result.value.open);
      }
    }
  }

  /** Takes the wish the backend keeps (once: no other view gets it) and does it. `null`: there was none. */
  async #takeIntent(): Promise<NotesIntent | null> {
    const taken = await this.#client.call("notes.takeIntent", {});
    if (!taken.ok || taken.value === null || this.#workspace === null) return null;
    const asked = taken.value;
    switch (asked.kind) {
      case "new":
        await this.newNote();
        break;
      case "open":
        await this.select(asked.path);
        break;
      default:
        asked satisfies never;
    }
    return asked;
  }

  /** The folder the notes are in is back or changed: read everything again. */
  async reset(): Promise<void> {
    this.list = { status: "loading" };
    await this.refresh();
  }

  // ---- the note shown --------------------------------------------------------------------------------------------

  /** Shows a document and puts the editor on it. `false`: the person stayed on the one being edited. */
  async select(path: string | null): Promise<boolean> {
    if (path === this.current) return true;
    if (path !== null && !(await confirmLeave({ kind: "document", path }))) return false;
    if (path === null && !(await confirmLeave({ kind: "view" }))) return false;
    this.current = path;
    const entry = path === null ? null : (this.entries.find((e) => e.path === path) ?? null);
    this.frozen =
      entry !== null && entry.kind === "note"
        ? { path: entry.path, modifiedMs: entry.modifiedMs }
        : null;
    void this.#client.call("notes.opened", { path });
    if (path !== null) await this.#open(path);
    return true;
  }

  async #open(path: string): Promise<void> {
    await this.#documents.show(path);
    if (this.current !== path) return;
    await this.#edit(path);
  }

  /** Notes are for writing: the editor opens on the note, in the mode last used. */
  async #edit(path: string): Promise<void> {
    const view = this.#documents.view;
    if (view === null || this.#documents.shown !== path || view.editable.editable === "readOnly") {
      return;
    }
    const kind = view.format === "markdown" ? "markdown" : "text";
    const mode = this.#editing.lastEditOr(this.#window.editMode);
    await this.#editing.enter(mode, path, kind, this.#documents.savePolicy);
  }

  /** A new empty note, shown and ready for typing. */
  async newNote(): Promise<void> {
    const made = await this.#client.call("notes.create", {});
    if (!made.ok) {
      this.#fail(made.error);
      return;
    }
    await this.refresh();
    this.filter = "";
    await this.select(made.value);
  }

  /** The next (`1`) or previous (`-1`) document in the list as drawn. */
  async step(direction: -1 | 1): Promise<void> {
    const next = neighbour(
      this.drawn().map((e) => e.path),
      this.current,
      direction,
    );
    if (next !== null) await this.select(next);
  }

  /** Ctrl+1..9. */
  async openPinned(n: number): Promise<void> {
    const entry = pinnedAt(
      this.entries.filter((e) => e.pinned),
      n,
    );
    if (entry !== null) await this.select(entry.path);
  }

  // ---- pins, share, copy -----------------------------------------------------------------------------------------

  async togglePin(path: string): Promise<void> {
    const entry = this.entries.find((e) => e.path === path);
    if (!entry) return;
    const result = await this.#client.call("notes.setPinned", { path, pinned: !entry.pinned });
    if (!result.ok) this.#fail(result.error);
    await this.refresh();
  }

  #pinOrder(): string[] {
    return this.entries.filter((e) => e.pinned).map((e) => e.path);
  }

  async movePin(path: string, direction: -1 | 1): Promise<void> {
    await this.#setOrder(movePinned(this.#pinOrder(), path, direction));
  }

  async dropPin(path: string, target: string): Promise<void> {
    await this.#setOrder(dropPinned(this.#pinOrder(), path, target));
  }

  async #setOrder(order: string[]): Promise<void> {
    const result = await this.#client.call("notes.setPinOrder", { order });
    if (!result.ok) this.#fail(result.error);
    await this.refresh();
  }

  async toggleShare(path: string): Promise<void> {
    const entry = this.entries.find((e) => e.path === path);
    if (!entry || entry.kind !== "note") return;
    const result = await this.#client.call("notes.setShared", { path, shared: !entry.shared });
    if (!result.ok) {
      this.#fail(result.error);
      return;
    }
    this.#window.notifyNote(
      t(entry.shared ? "notes.sharedOff" : "notes.sharedOn", { name: shownName(entry) }),
    );
    await this.refresh();
  }

  /** The note's text, exactly as it is, on the clipboard; it is marked until it is edited. */
  async copyNote(path: string): Promise<void> {
    const entry = this.entries.find((e) => e.path === path);
    if (!entry) return;
    const typed = this.#editing.path === path ? this.#editing.text : null;
    let text = typed;
    if (text === null) {
      const read = await this.#api.documentText(path);
      if (!read.ok) {
        this.#fail(read.error);
        return;
      }
      text = read.value;
    }
    if (text.trim() === "") {
      this.#window.notifyNote(t("notes.copyEmpty"));
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      this.#window.notifyNote(t("comments.copyFailed"));
      return;
    }
    if (entry.kind === "note") void this.#client.call("notes.markCopied", { path });
    this.#window.notifyNote(tn("notes.copied", [...text].length));
    await this.refresh();
  }

  get copiedCount(): number {
    return this.entries.filter((e) => e.copied && !e.pinned).length;
  }

  async deleteCopied(): Promise<void> {
    const result = await this.#client.call("notes.deleteCopied", {});
    if (!result.ok) {
      this.#fail(result.error);
      return;
    }
    await this.#afterDelete(result.value.names, result.value.undoId, "notes.deletedCopied");
  }

  // ---- delete, rename -------------------------------------------------------------------------------------------------

  /** Deletes notes; pinned ones are asked about first. */
  async deleteNotes(paths: readonly string[]): Promise<void> {
    const pinned = paths.filter((p) => this.entries.find((e) => e.path === p)?.pinned);
    if (pinned.length > 0) {
      this.confirm = { status: "deleteNotes", paths, pinned };
      return;
    }
    await this.#delete(paths);
  }

  async confirmDelete(): Promise<void> {
    const asked = this.confirm;
    this.confirm = { status: "closed" };
    if (asked.status === "deleteNotes") await this.#delete(asked.paths);
    else if (asked.status === "removeFile") await this.#remove(asked.path);
  }

  cancelConfirm(): void {
    this.confirm = { status: "closed" };
  }

  async #delete(paths: readonly string[]): Promise<void> {
    const wasCurrent = this.current !== null && paths.includes(this.current);
    // the note is going: what is being typed into it must not save it back
    if (wasCurrent) this.#editing.release();
    const result = await this.#client.call("notes.delete", { paths: [...paths] });
    if (!result.ok) {
      this.#fail(result.error);
      if (wasCurrent && this.current !== null) await this.#open(this.current);
      return;
    }
    await this.#afterDelete(result.value.names, result.value.undoId, "notes.deleted");
  }

  async #afterDelete(
    names: readonly string[],
    undoId: number | null,
    key: "notes.deleted" | "notes.deletedCopied",
  ): Promise<void> {
    const before = this.drawn().map((e) => e.path);
    const wasAt = this.current === null ? -1 : before.indexOf(this.current);
    // the words for the notice come from the row that is about to go
    const gone = this.entries.find((e) => e.path === names[0]);
    const label = gone ? shownName(gone) : stem(names[0] ?? "");
    await this.refresh();
    if (this.current !== null && names.includes(this.current)) {
      const rest = this.drawn().map((e) => e.path);
      const next =
        rest.length === 0 ? null : (rest[Math.min(Math.max(wasAt, 0), rest.length - 1)] ?? null);
      this.current = null;
      this.frozen = null;
      await this.select(next);
    }
    if (names.length === 0) return;
    const text =
      key === "notes.deleted" && names.length === 1
        ? t("notes.deleted.one", { name: label })
        : tn(key, names.length);
    if (undoId === null) this.#window.notifyNote(text);
    else {
      this.#window.notifyAction(text, {
        label: t("notes.undo"),
        run: () => void this.undoDelete(undoId),
      });
    }
  }

  async undoDelete(undoId: number): Promise<void> {
    const result = await this.#client.call("notes.undoDelete", { undoId });
    if (!result.ok) {
      this.#fail(result.error);
      return;
    }
    await this.refresh();
    const first = result.value[0];
    if (first !== undefined) await this.select(first);
  }

  /** Renames a note (its file) or a loose file. Resolves to an error word for the field, `null` when done. */
  async rename(path: string, name: string): Promise<string | null> {
    const entry = this.entries.find((e) => e.path === path);
    if (!entry) return null;
    const trimmed = name.trim();
    if (trimmed === "") return t("notes.rename.empty");
    const wasCurrent = this.current === path;
    // the text typed so far is saved before the file changes its name
    if (
      wasCurrent &&
      this.#editing.path === path &&
      !(await this.#editing.confirmLeave({ kind: "view" }))
    ) {
      return null;
    }
    const result = await this.#client.call("notes.rename", { path, name: trimmed });
    if (!result.ok) {
      if (wasCurrent) await this.#edit(path);
      if (result.error.kind === "backend") {
        const error = result.error.error;
        if (error.kind === "exists") return t("notes.rename.exists", { name: trimmed });
        if (error.kind === "invalidName") return invalidNameText(error.detail);
      }
      this.#fail(result.error);
      return null;
    }
    if (wasCurrent) {
      this.current = result.value;
      this.frozen = null;
    }
    await this.refresh();
    if (wasCurrent) {
      void this.#client.call("notes.opened", { path: result.value });
      await this.#open(result.value);
    }
    return null;
  }

  // ---- versions ----------------------------------------------------------------------------------------------------------

  async openVersions(path: string): Promise<void> {
    this.versions = { status: "open", path, items: null, selected: null, preview: null };
    const result = await this.#client.call("notes.versions", { path });
    if (this.versions.status !== "open" || this.versions.path !== path) return;
    if (!result.ok) {
      this.versions = { status: "closed" };
      this.#fail(result.error);
      return;
    }
    this.versions = { status: "open", path, items: result.value, selected: null, preview: null };
    const newest = result.value[0];
    if (newest !== undefined) await this.selectVersion(newest.id);
  }

  async selectVersion(id: string): Promise<void> {
    const shown = this.versions;
    if (shown.status !== "open") return;
    this.versions = { ...shown, selected: id, preview: null };
    const result = await this.#client.call("notes.versionText", { path: shown.path, id });
    const now = this.versions;
    if (now.status !== "open" || now.selected !== id) return;
    this.versions = { ...now, preview: result.ok ? result.value : errorOf(result) };
  }

  closeVersions(): void {
    this.versions = { status: "closed" };
  }

  async restoreVersion(): Promise<void> {
    const shown = this.versions;
    if (shown.status !== "open" || shown.selected === null) return;
    const { path, selected } = shown;
    const wasCurrent = this.current === path;
    if (
      wasCurrent &&
      this.#editing.path === path &&
      !(await this.#editing.confirmLeave({ kind: "view" }))
    ) {
      return;
    }
    const result = await this.#client.call("notes.restoreVersion", { path, id: selected });
    this.versions = { status: "closed" };
    if (!result.ok) {
      this.#fail(result.error);
      return;
    }
    this.#window.notifyNote(t("notes.restored"));
    await this.refresh();
    if (wasCurrent) {
      this.#documents.reload();
      await this.#open(path);
    }
  }

  // ---- move to a project -----------------------------------------------------------------------------------------------

  async openMove(path: string): Promise<void> {
    const recents = await this.#api.recentList();
    const list = recents.ok ? recents.value.filter((r) => !r.missing) : [];
    const first = list[0]?.path ?? null;
    const name = await this.#suggest(path, first);
    this.move = {
      status: "open",
      path,
      folder: first,
      name,
      recents: list,
      error: null,
      busy: false,
    };
  }

  async #suggest(path: string, folder: string | null): Promise<string> {
    const result = await this.#client.call("notes.suggestName", { path, folder });
    return result.ok ? result.value : "";
  }

  async chooseMoveFolder(folder: string | null): Promise<void> {
    const shown = this.move;
    if (shown.status !== "open" || folder === null) return;
    const name = await this.#suggest(shown.path, folder);
    const now = this.move;
    if (now.status === "open") this.move = { ...now, folder, name, error: null };
  }

  async browseMoveFolder(): Promise<void> {
    const picked = await this.#api.pickFolder(t("notes.move.browseTitle"));
    if (picked !== null) await this.chooseMoveFolder(picked);
  }

  setMoveName(name: string): void {
    if (this.move.status === "open") this.move = { ...this.move, name, error: null };
  }

  closeMove(): void {
    this.move = { status: "closed" };
  }

  async confirmMove(): Promise<void> {
    const shown = this.move;
    if (shown.status !== "open" || shown.folder === null || shown.busy) return;
    this.move = { ...shown, busy: true, error: null };
    const { path, folder, name } = shown;
    const wasCurrent = this.current === path;
    // what was typed is saved first: the copy is what the note holds
    if (
      wasCurrent &&
      this.#editing.path === path &&
      !(await this.#editing.confirmLeave({ kind: "view" }))
    ) {
      this.move = { ...shown, busy: false };
      return;
    }
    const entry = this.entries.find((e) => e.path === path);
    const result = await this.#client.call("notes.moveToProject", { path, folder, name });
    if (!result.ok) {
      if (wasCurrent) await this.#edit(path);
      this.move = { ...shown, busy: false, error: this.#moveError(result.error, name) };
      return;
    }
    this.move = { status: "closed" };
    const moved = result.value;
    const label = entry ? shownName(entry) : name;
    if (moved.kept) {
      this.#window.notifyNote(t("notes.moveKept", { name: label }));
      if (wasCurrent) await this.#open(path);
      return;
    }
    await this.#afterMove(path, moved.destination, moved.undoId, label, folder);
  }

  #moveError(error: ApiError, name: string): string {
    if (error.kind === "backend") {
      const kind = error.error.kind;
      if (kind === "exists") return t("notes.move.exists", { name });
      if (kind === "intoItself") return t("notes.move.inside");
      if (kind === "invalidName") return invalidNameText(error.error.detail);
    }
    return apiErrorText(error);
  }

  async #afterMove(
    path: string,
    destination: string,
    undoId: number | null,
    label: string,
    folder: string,
  ): Promise<void> {
    if (this.current === path) this.#editing.release();
    const before = this.drawn().map((e) => e.path);
    const wasAt = before.indexOf(path);
    await this.refresh();
    if (this.current === path) {
      const rest = this.drawn().map((e) => e.path);
      this.current = null;
      this.frozen = null;
      await this.select(
        rest.length === 0 ? null : (rest[Math.min(Math.max(wasAt, 0), rest.length - 1)] ?? null),
      );
    }
    const text = t("notes.moved", { name: label, folder: lastSegment(folder) });
    const actions = [
      { label: t("notes.moved.open"), run: () => void this.#api.openInProject(destination) },
    ];
    if (undoId !== null) {
      actions.unshift({ label: t("notes.undo"), run: () => void this.undoMove(undoId, label) });
    }
    this.#window.notifyAction(text, ...actions);
  }

  async undoMove(undoId: number, label: string): Promise<void> {
    const result = await this.#client.call("notes.undoMove", { undoId });
    if (!result.ok) {
      this.#fail(result.error);
      return;
    }
    this.#window.notifyNote(
      t(result.value.removed ? "notes.moveUndone" : "notes.moveUndoneKept", { name: label }),
    );
    await this.refresh();
    await this.select(result.value.note);
  }

  // ---- recovery --------------------------------------------------------------------------------------------------------------

  async openCompare(path: string): Promise<void> {
    const kept = await this.#client.call("notes.recoveredText", { path });
    const disk = await this.#api.editSource(path);
    if (!kept.ok || kept.value === null) return;
    this.compare = {
      status: "open",
      path,
      mine: kept.value,
      theirs: disk.ok ? disk.value.text : "",
    };
  }

  closeCompare(): void {
    this.compare = { status: "closed" };
  }

  /** Keep the recovered text: it is written over the note. */
  async keepRecovered(path: string): Promise<void> {
    this.compare = { status: "closed" };
    const wasCurrent = this.current === path;
    if (wasCurrent && this.#editing.path === path) this.#editing.release();
    const result = await this.#client.call("notes.keepRecovered", { path });
    if (!result.ok) this.#fail(result.error);
    await this.refresh();
    if (wasCurrent) {
      this.#documents.reload();
      await this.#open(path);
    }
  }

  async discardRecovered(path: string): Promise<void> {
    this.compare = { status: "closed" };
    await this.#client.call("notes.discardRecovered", { path });
    await this.refresh();
  }

  // ---- loose files -----------------------------------------------------------------------------------------------------------

  async chooseFile(): Promise<void> {
    const picked = await this.#api.pickFile(
      t("notes.openFileTitle"),
      t("dialog.filter.docs"),
      true,
    );
    if (picked !== null) await this.openFiles([picked]);
  }

  /** Lists files (a dialog, a drop) and shows the last. */
  async openFiles(paths: readonly string[]): Promise<void> {
    let last: string | null = null;
    for (const path of paths) {
      const result = await this.#client.call("notes.openFile", { path });
      if (result.ok) last = result.value;
      else this.#fail(result.error);
    }
    await this.refresh();
    if (last !== null) await this.select(last);
  }

  /** The person takes the file's own version; what was typed is let go. */
  async reloadFile(path: string): Promise<void> {
    if (this.current === path) this.#editing.release();
    const result = await this.#client.call("notes.reloadFile", { path });
    if (!result.ok) this.#fail(result.error);
    await this.refresh();
    if (this.current === path) {
      this.#documents.reload();
      await this.#open(path);
    }
  }

  /** The person keeps what was typed: it is written over the file. */
  async keepMine(path: string): Promise<void> {
    if (this.current === path && this.#editing.path === path) {
      this.#editing.dispatch({ e: "keepMine" });
      return;
    }
    const result = await this.#client.call("notes.keepMine", { path });
    if (!result.ok) this.#fail(result.error);
    await this.refresh();
    if (this.current === path) {
      this.#documents.reload();
      await this.#open(path);
    }
  }

  async saveAsNote(path: string): Promise<void> {
    const result = await this.#client.call("notes.saveAsNote", { path });
    if (!result.ok) {
      this.#fail(result.error);
      return;
    }
    this.#window.notifyNote(t("notes.file.editsKept", { name: stem(result.value) }));
    await this.refresh();
    await this.select(result.value);
  }

  async locateFile(path: string): Promise<void> {
    const picked = await this.#api.pickFile(
      t("notes.openFileTitle"),
      t("dialog.filter.docs"),
      true,
    );
    if (picked === null) return;
    const result = await this.#client.call("notes.locateFile", { path, newPath: picked });
    if (!result.ok) {
      this.#fail(result.error);
      return;
    }
    const wasCurrent = this.current === path;
    if (wasCurrent) this.current = result.value;
    await this.refresh();
    if (wasCurrent) await this.#open(result.value);
  }

  /** Takes a file off the list (asks first when typed text would go with it). */
  async removeFile(path: string): Promise<void> {
    const entry = this.entries.find((e) => e.path === path);
    if (entry?.pending === true) {
      this.confirm = { status: "removeFile", path };
      return;
    }
    await this.#remove(path);
  }

  async #remove(path: string): Promise<void> {
    const entry = this.entries.find((e) => e.path === path);
    if (this.current === path) this.#editing.release();
    const result = await this.#client.call("notes.removeFile", { path });
    if (!result.ok) {
      this.#fail(result.error);
      return;
    }
    if (this.current === path) {
      this.current = null;
      this.frozen = null;
    }
    await this.refresh();
    this.#window.notifyNote(t("notes.file.removed", { name: entry?.name ?? "" }));
  }

  /** The real place of a document, on the clipboard. */
  async copyPath(path: string): Promise<void> {
    const place = await this.#api.absPath(path);
    if (!place.ok) {
      this.#fail(place.error);
      return;
    }
    try {
      await navigator.clipboard.writeText(place.value);
    } catch {
      this.#window.notifyNote(t("comments.copyFailed"));
    }
  }

  /** Shows the file in the system's file manager. */
  async reveal(path: string): Promise<void> {
    const result = await this.#api.openExternal(path, "reveal");
    if (!result.ok) this.#fail(result.error);
  }

  /** Opens the project that holds a loose file in a window of its own. */
  async openInProject(path: string): Promise<void> {
    const place = await this.#api.absPath(path);
    if (!place.ok) {
      this.#fail(place.error);
      return;
    }
    const opened = await this.#api.openInProject(place.value);
    if (!opened.ok) this.#fail(opened.error);
  }

  // ---- the folder ------------------------------------------------------------------------------------------------------------

  async createFolder(): Promise<void> {
    const result = await this.#client.call("notes.createFolder", {});
    if (!result.ok) this.#fail(result.error);
    await this.reset();
  }

  async locateFolder(): Promise<void> {
    const picked = await this.#api.pickFolder(t("notes.settings.pickTitle"));
    if (picked === null) return;
    await this.#window.changeSetting({ key: "notesFolder", value: picked });
    await this.reset();
  }
}

/** The file name without its extension. */
function stem(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

function lastSegment(path: string): string {
  const parts = path.split(/[\\/]+/).filter((p) => p !== "");
  return parts.at(-1) ?? path;
}

// ---- asking from any view -------------------------------------------------------------------------------------------

/**
 * Shows a note | a new note in the Notes tab, wherever it is: the wish goes to the backend first (the Notes tab takes it
 * once, even in another tab or window, even while it loads), then the Notes tab opens here or comes forward.
 */
export async function showInNotes(
  intent: NotesIntent,
  deps: { client?: PluginClient; window?: WindowStore } = {},
): Promise<void> {
  const client = deps.client ?? pluginClient;
  const window = deps.window ?? defaultWindow;
  await client.call("notes.want", { intent });
  await window.openNotes("tab");
}

export const notes = new NotesStore();
