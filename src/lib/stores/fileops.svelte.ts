// File operations in the Explorer (docs/design/v0.2.md section 17): rename, move (drag and drop, cut and paste),
// delete to the recycle bin, new folders, and undo. This store holds what the tree shows while it happens (the
// selection, the name being typed, the drag in progress, the "update links?" question) and carries each operation
// through: ask the backend, follow the move in the other stores, report it as one feed row and one toast.

import { SvelteSet } from "svelte/reactivity";
import { api } from "../api";
import { errorKind, errorText } from "../errors";
import {
  UndoStack,
  deleteSummary,
  dropProblem,
  isRootPath,
  joinPath,
  isRename,
  linkDecision,
  mapPath,
  moveSummary,
  movesFor,
  renamedPath,
  shouldScanLinks,
  skipNotes,
  topLevelOnly,
} from "../fileOps";
import { t, tn } from "../i18n";
import { basename } from "../paths";
import { askYesNo } from "../platform";
import { isDirPath } from "../tree";
import type { BufferText, Move, MoveOutcome, MovePreview } from "../types";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { feed } from "./feed.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";
import { vault } from "./vault.svelte";

/** What the person answered to "Update N links in M files?". */
export type LinkChoice = "apply" | "moveOnly" | "cancel";

/** The question being asked: what would change, and how to answer. */
export type LinkPrompt = { preview: MovePreview; answer: (choice: LinkChoice) => void };

/** A drag in progress: what is dragged and where a drop would land. */
export type DragState = {
  sources: string[];
  /** The folder that would receive the drop ("" is the project root), or null when the pointer is not over a valid one. */
  over: string | null;
};

class FileOpsStore {
  /** The rows picked with Ctrl and Shift (a plain click picks just one). */
  selection = new SvelteSet<string>();
  /** The row a Shift+click extends the selection from. */
  anchor: string | null = null;
  /** Rows marked with Ctrl+X, waiting for a paste. */
  cut = new SvelteSet<string>();
  /** Folders made here this session: the Explorer filter keeps them while they are empty. */
  madeFolders = new SvelteSet<string>();
  /** The row whose name is being typed over (`n` makes each request new). */
  renaming = $state.raw<{ path: string; n: number } | null>(null);
  /** The folder a new folder's name is being typed in. */
  newFolder = $state.raw<{ folder: string; n: number } | null>(null);
  drag = $state.raw<DragState | null>(null);
  /** Where the pointer is during a drag (its own state: it changes with every move, `drag` does not). */
  dragPoint = $state.raw({ x: 0, y: 0 });
  prompt = $state.raw<LinkPrompt | null>(null);
  /** An operation is running: the next one waits for it. */
  busy = $state(false);

  private undoStack = new UndoStack();
  private renameCount = 0;
  private folderCount = 0;

  /** Forgets everything (another project was opened). */
  reset(): void {
    this.prompt?.answer("cancel");
    this.selection.clear();
    this.anchor = null;
    this.cut.clear();
    this.renaming = null;
    this.newFolder = null;
    this.drag = null;
    this.busy = false;
    this.undoStack.clear();
    this.madeFolders.clear();
  }

  // ---- selection ------------------------------------------------------------------------------

  pick(paths: Iterable<string>, anchor: string | null): void {
    this.selection.clear();
    for (const path of paths) this.selection.add(path);
    this.anchor = anchor;
  }

  clearPicked(): void {
    this.selection.clear();
    this.cut.clear();
  }

  // ---- rename ---------------------------------------------------------------------------------

  startRename(path: string): void {
    if (isRootPath(path) || vault.isSimple) return;
    this.newFolder = null;
    this.renaming = { path, n: ++this.renameCount };
  }

  cancelRename(): void {
    this.renaming = null;
  }

  /**
   * Gives an entry a new name. Resolves to the message to show under the name field when it cannot be done, else
   * null (done, or the person cancelled the question about links).
   */
  async rename(path: string, name: string): Promise<string | null> {
    const to = renamedPath(path, name);
    if (to === path) {
      this.renaming = null;
      return null;
    }
    const failure = await this.run([{ from: path, to }]);
    if (failure === null) this.renaming = null;
    return failure;
  }

  // ---- move -----------------------------------------------------------------------------------

  /** Moves entries into a folder ("" is the project root). Failures are shown as a toast. */
  async move(sources: readonly string[], folder: string): Promise<void> {
    const moves = movesFor(sources, folder).filter((m) => m.from !== m.to);
    if (moves.length === 0) return;
    const failure = await this.run(moves);
    if (failure !== null) toast.message(t("fileOps.moveFailed", { message: failure }), "warn");
  }

  /** Ctrl+X: marks entries to move with the next paste. */
  cutPicked(paths: readonly string[]): void {
    this.cut.clear();
    for (const path of topLevelOnly(paths).filter((p) => !isRootPath(p))) this.cut.add(path);
  }

  /** Ctrl+V: moves what was cut into a folder. */
  async paste(folder: string): Promise<void> {
    const sources = [...this.cut];
    if (sources.length === 0) return;
    if (dropProblem(sources, folder) !== null) return;
    this.cut.clear();
    await this.move(sources, folder);
  }

  /** The preview, the question about links, the move; and then everything that follows a move. */
  private async run(moves: Move[]): Promise<string | null> {
    if (this.busy) return null;
    if (vault.isSimple) return t("fileOps.simpleMode");
    this.busy = true;
    try {
      const dirs = moves.map((m) => isDirPath(vault.tree, m.from));
      const setting = session.linkUpdate;
      let preview: MovePreview;
      try {
        preview = await api.previewMove(moves, shouldScanLinks(setting));
      } catch (error) {
        return errorText(error);
      }
      const decision = linkDecision(setting, preview);
      let updateLinks = decision === "update";
      if (decision === "ask") {
        const choice = await this.ask(preview);
        if (choice === "cancel") return null;
        updateLinks = choice === "apply";
      }
      // a document with unsaved edits is not written: the changes for its text come back instead
      const buffer = updateLinks ? edit.unsavedBuffer() : null;
      let outcome: MoveOutcome;
      try {
        outcome = await api.applyMove(preview.planId, updateLinks, buffer ? [buffer] : []);
      } catch (error) {
        return errorText(error);
      }
      await this.afterMove(outcome, dirs, buffer);
      return null;
    } finally {
      this.busy = false;
    }
  }

  private ask(preview: MovePreview): Promise<LinkChoice> {
    return new Promise((resolve) => {
      this.prompt = {
        preview,
        answer: (choice) => {
          this.prompt = null;
          resolve(choice);
        },
      };
    });
  }

  /** The buttons of the question (and Esc). */
  answer(choice: LinkChoice): void {
    this.prompt?.answer(choice);
  }

  /** The move is made: the stores follow it, and it is reported once. */
  private async afterMove(
    outcome: MoveOutcome,
    dirs: boolean[],
    buffer: BufferText | null,
  ): Promise<void> {
    const moves = outcome.moved;
    await vault.refreshTree();
    this.follow(moves);
    const changes = buffer ? outcome.buffers.find((b) => b.path === buffer.path) : undefined;
    const applied = edit.followMoves(moves, changes, buffer?.text);
    await docs.followMoves(moves);
    vault.revealPath(moves[0].to);

    const summary = moveSummary(moves, dirs, outcome.linksUpdated);
    const notes = skipNotes(outcome.skipped);
    if (!applied && buffer) {
      notes.push(t("fileOps.bufferNotUpdated", { name: basename(buffer.path) }));
    }
    feed.addOperation({
      kind: isRename(moves) ? "renamed" : "moved",
      atMs: Date.now(),
      summary,
      items: moves.map((m) => ({ path: m.to, from: m.from })),
      notes,
    });
    this.undoStack.push({ opId: outcome.opId, label: summary, moves, folders: [] });
    const left = outcome.skipped.length + (applied ? 0 : 1);
    const text = left > 0 ? `${summary}. ${tn("fileOps.notUpdated", left)}` : summary;
    toast.message(text, left > 0 ? "warn" : "info", {
      label: t("fileOps.undo"),
      run: () => void this.undo(outcome.opId),
    });
  }

  /** The stores that keep paths follow a move (or an undo of one). */
  private follow(moves: readonly Move[]): void {
    vault.followMoves(moves);
    session.followMoves(moves);
    this.pick(
      [...this.selection].map((p) => mapPath(p, moves)),
      this.anchor === null ? null : mapPath(this.anchor, moves),
    );
    const made = [...this.madeFolders].map((p) => mapPath(p, moves));
    this.madeFolders.clear();
    for (const path of made) this.madeFolders.add(path);
    this.cut.clear();
  }

  // ---- delete ---------------------------------------------------------------------------------

  /** Del: asks, naming how many, and sends the entries to the recycle bin. */
  async remove(paths: readonly string[]): Promise<void> {
    const items = topLevelOnly(paths).filter((p) => !isRootPath(p));
    if (items.length === 0 || this.busy || vault.isSimple) return;
    const dirs = items.map((p) => isDirPath(vault.tree, p));
    const question =
      items.length > 1
        ? tn("fileOps.deleteConfirm", items.length)
        : dirs[0]
          ? t("fileOps.deleteFolderConfirm", { name: basename(items[0]) })
          : tn("fileOps.deleteConfirm", 1, { name: basename(items[0]) });
    if (!(await askYesNo(question, t("fileOps.deleteButton"), t("common.cancel")))) return;
    this.busy = true;
    try {
      const outcome = await api.deleteEntries(items);
      await vault.refreshTree();
      this.selection.clear();
      this.cut.clear();
      docs.followRemoved(outcome.deleted);
      edit.followRemoved();
      const summary = deleteSummary(dirs);
      feed.addOperation({
        kind: "deleted",
        atMs: Date.now(),
        summary,
        items: outcome.deleted.map((path) => ({ path })),
        notes: [],
      });
      toast.message(summary);
    } catch (error) {
      toast.message(t("fileOps.deleteFailed", { message: errorText(error) }), "warn");
    } finally {
      this.busy = false;
    }
  }

  // ---- new folder -----------------------------------------------------------------------------

  /** "New folder...": the name is typed in the tree, under `folder`. */
  startNewFolder(folder: string): void {
    if (vault.isSimple) {
      toast.message(t("fileOps.simpleMode"), "info");
      return;
    }
    this.renaming = null;
    ui.sidebarView = "explorer";
    if (folder !== "") vault.expanded.add(folder);
    this.newFolder = { folder, n: ++this.folderCount };
  }

  cancelNewFolder(): void {
    this.newFolder = null;
  }

  /** Makes the folder. Resolves to the message for the name field when it cannot be made, else null. */
  async createFolder(folder: string, name: string): Promise<string | null> {
    const path = joinPath(folder, name);
    try {
      const made = await api.newFolder(path);
      this.newFolder = null;
      this.madeFolders.add(made.path);
      await vault.refreshTree();
      vault.revealPath(made.path);
      const summary = t("fileOps.summaryFolder", { name });
      this.undoStack.push({ opId: made.opId, label: summary, moves: [], folders: [made.path] });
      feed.addOperation({
        kind: "folder",
        atMs: Date.now(),
        summary,
        items: [{ path: made.path }],
        notes: [],
      });
      return null;
    } catch (error) {
      return errorKind(error) === "exists" ? t("fileOps.nameExists") : errorText(error);
    }
  }

  // ---- undo -----------------------------------------------------------------------------------

  /** Ctrl+Z in the Explorer and the toast's Undo: the last operation (or the one given) is put back. */
  async undo(opId?: number): Promise<void> {
    if (this.busy) return;
    const entry = opId === undefined ? this.undoStack.pop() : this.undoStack.remove(opId);
    if (!entry) {
      toast.message(t("fileOps.nothingToUndo"));
      return;
    }
    this.busy = true;
    try {
      const outcome = await api.undoFileOp(entry.opId);
      for (const folder of entry.folders) this.madeFolders.delete(folder);
      await vault.refreshTree();
      this.follow(outcome.movedBack);
      edit.followMoves(outcome.movedBack);
      await docs.followMoves(outcome.movedBack);
      const summary = t("fileOps.summaryUndone", { summary: entry.label });
      feed.addOperation({
        kind: "undone",
        atMs: Date.now(),
        summary,
        items: outcome.movedBack.map((m) => ({ path: m.to, from: m.from })),
        notes: skipNotes(outcome.couldNot),
      });
      const left = outcome.couldNot.length;
      toast.message(
        left > 0 ? `${summary}. ${tn("fileOps.couldNotUndo", left)}` : summary,
        left > 0 ? "warn" : "info",
      );
    } catch (error) {
      toast.message(t("fileOps.undoFailed", { message: errorText(error) }), "warn");
    } finally {
      this.busy = false;
    }
  }
}

export const fileops = new FileOpsStore();
