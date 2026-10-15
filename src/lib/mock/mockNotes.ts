// The Notes half of the browser mock: the same commands as `notes_commands.rs`, over a few notes kept in memory. The
// notes are stored in the mock documents' "written" layer (see `fixtures.ts`), so reading and editing them goes through the
// ordinary mock commands; this module adds what the Notes window needs on top: names, pins, versions, copies, deleting.
//
// `?notes` starts the browser mock as the Notes window; `?notes=missing` shows the folder that is not there, and
// `?notes=locked` shows the notes of a second window (read-only).

import { emitMock } from "../api";
import { isBlank, noteTitle } from "../noteText";
import type {
  CreatedDoc,
  DocumentView,
  LaunchMode,
  LooseStatus,
  NoteDeleted,
  NoteEntry,
  NoteList,
  NoteRestored,
  NoteSaved,
  NotesFolder,
  NotesOpened,
  NotesOverview,
  NotesReadOnly,
  NotesRequest,
  NotesSummary,
  NoteVersion,
  NoteView,
  OpenedFile,
  PathClass,
  PendingEdits,
  SourceDoc,
  StartIn,
  VaultInfo,
} from "../types";
import { mockFiles, mockNoteFiles, mockState, sourceMtime, sourceText } from "./fixtures";
import { mockParseBlocks } from "./mockBlocks";
import { markdownToView } from "./mockMarkdown";
import { MockBackendError, mockHash } from "./mockSource";

const MINUTE = 60 * 1000;
const DAY = 24 * 60 * MINUTE;
const VERSION_GAP_MS = 10 * MINUTE;

const FOLDER = "C:/Users/demo/Documents/TrailMark Notes";

const flag = (): string | null =>
  typeof location === "undefined" ? null : new URLSearchParams(location.search).get("notes");

/** A loose file of the mock: a file of the disk that is not in the notes folder. */
type MockLoose = {
  id: string;
  name: string;
  folder: string;
  openedMs: number;
  missing: boolean;
  /** Someone else changed it since it was last read. */
  changed: boolean;
};

const notes = {
  seeded: false,
  loose: [] as MockLoose[],
  pending: new Map<string, PendingEdits>(),
  folderMissing: false,
  startIn: "welcome" as StartIn,
  pins: [] as string[],
  open: null as string | null,
  views: new Map<string, NoteView>(),
  /** The modification time a note had when it was copied. */
  copied: new Map<string, number>(),
  /** New notes that have a name and no file yet. */
  reserved: new Set<string>(),
  versions: new Map<string, { id: string; writtenMs: number; text: string }[]>(),
  lastVersionAt: new Map<string, number>(),
  deleted: new Map<number, { name: string; text: string; mtime: number }[]>(),
  nextUndo: 1,
  request: null as NotesRequest | null,
};

function putNote(name: string, text: string, mtime: number): void {
  mockNoteFiles.add(name);
  mockFiles.written.set(name, { text, mtime });
  mockState.modified.set(name, mtime);
  mockState.deleted.delete(name);
}

function seed(): void {
  if (notes.seeded) return;
  notes.seeded = true;
  const now = Date.now();
  const make = (name: string, text: string, age: number): void => putNote(name, text, now - age);
  make(
    "Server IPs and ports.md",
    "# Server IPs and ports\n\napi 10.0.0.4, db 10.0.0.5, cache 10.0.0.6\n",
    20 * DAY,
  );
  make(
    "Weekly checklist.md",
    "# Weekly checklist\n\n- [ ] backups\n- [ ] patches\n- [x] invoices\n",
    9 * DAY,
  );
  make(
    "2026-10-15 1432.md",
    "Ask the agent to add tests for the checkout storm.\n\nConstraints:\n- keep the block glow\n- do not touch the public API of `src/watcher.rs`\n",
    2 * MINUTE,
  );
  make(
    "2026-10-15 1105.md",
    "Refactor the watcher\n\nsplit the debounce from the filter\n",
    3 * 60 * MINUTE,
  );
  make(
    "2026-10-14 1810.md",
    "Release notes for 0.4\n\n- comments\n- the agent connection\n",
    DAY + 60 * MINUTE,
  );
  make("2026-10-12 0930.md", "Meeting scraps\n\nask about the budget\n", 3 * DAY);
  make("2026-09-02 1700.md", "An old idea about tags\n", 43 * DAY);
  notes.pins = ["Server IPs and ports.md", "Weekly checklist.md"];
  notes.copied.set("2026-10-15 1105.md", sourceMtime("2026-10-15 1105.md"));
  const sent = (
    id: string,
    name: string,
    folder: string,
    text: string,
    openedAgo: number,
    missing = false,
  ): void => {
    const loose: MockLoose = {
      id,
      name,
      folder,
      openedMs: now - openedAgo,
      missing,
      changed: false,
    };
    notes.loose.push(loose);
    if (!missing) putNote(looseUi(loose), text, now - openedAgo - DAY);
  };
  sent(
    "0a1b2c3d",
    "proposal.md",
    "~\\Downloads",
    "# Proposal\n\nThe plan for the next release, as it came in an email.\n\n- scope\n- dates\n",
    90 * MINUTE,
  );
  sent("1b2c3d4e", "todo.txt", "~\\Documents", "buy milk\nrenew the passport\n", 5 * DAY);
  sent("2c3d4e5f", "old-plan.md", "D:\\Archive", "", 30 * DAY, true);
}

const looseUi = (file: MockLoose): string => `@file/${file.id}/${file.name}`;
const looseFile = (path: string): MockLoose | undefined =>
  notes.loose.find((file) => path.startsWith(`@file/${file.id}/`));

function looseEntry(file: MockLoose): NoteEntry {
  const path = looseUi(file);
  const text = file.missing ? "" : textOf(path);
  return {
    kind: "file",
    path,
    title: file.name,
    snippet: "",
    modifiedMs: file.missing ? 0 : sourceMtime(path),
    sizeBytes: new TextEncoder().encode(text).length,
    pinned: notes.pins.includes(path),
    copied: false,
    view: notes.views.get(path) ?? null,
    folder: file.folder,
    missing: file.missing,
    unsaved: notes.pending.has(path),
  };
}

const existing = (): string[] =>
  [...mockNoteFiles].filter(
    (name) => !mockState.deleted.has(name) && /^[^/]+\.md$/i.test(name) && !name.startsWith("."),
  );

const textOf = (name: string): string => sourceText(name) ?? "";

function entry(name: string): NoteEntry {
  const text = textOf(name);
  const lines = text.split(/\r?\n/);
  const firstAt = lines.findIndex((line) => noteTitle(line) !== "");
  const snippet = lines
    .slice(firstAt + 1)
    .map((line) => noteTitle(line))
    .filter((line) => line !== "")
    .join(" ")
    .slice(0, 160);
  const modifiedMs = sourceMtime(name);
  return {
    kind: "note",
    path: name,
    title: noteTitle(text),
    snippet,
    modifiedMs,
    sizeBytes: new TextEncoder().encode(text).length,
    pinned: notes.pins.includes(name),
    copied: notes.copied.get(name) === modifiedMs,
    view: notes.views.get(name) ?? null,
    folder: null,
    missing: false,
    unsaved: false,
  };
}

/** The notes, then the loose files (the one opened last first), the pinned ones of both first in the order they were pinned. */
function sortedEntries(): NoteEntry[] {
  const files = [...notes.loose].sort((a, b) => b.openedMs - a.openedMs).map(looseEntry);
  const all = [...existing().map(entry), ...files];
  const rank = (n: NoteEntry): number => notes.pins.indexOf(n.path);
  const kindRank = (n: NoteEntry): number => (n.kind === "note" ? 0 : 1);
  return all.sort((a, b) => {
    const [x, y] = [rank(a), rank(b)];
    if (x >= 0 && y >= 0) return x - y;
    if (x >= 0) return -1;
    if (y >= 0) return 1;
    if (a.kind !== b.kind) return kindRank(a) - kindRank(b);
    if (a.kind === "file") return 0;
    return b.modifiedMs - a.modifiedMs || a.path.localeCompare(b.path);
  });
}

function folder(): NotesFolder {
  return {
    path: FOLDER,
    status: notes.folderMissing || flag() === "missing" ? "missing" : "ready",
    isDefault: true,
  };
}

const pad = (n: number): string => String(n).padStart(2, "0");

function stamp(at = new Date()): string {
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())} ${pad(at.getHours())}${pad(at.getMinutes())}`;
}

function freeName(stem: string, separator: string): string {
  const taken = (name: string): boolean => existing().includes(name) || notes.reserved.has(name);
  for (let n = 1; n < 1000; n++) {
    const name = n === 1 ? `${stem}.md` : `${stem}${separator}${n}.md`;
    if (!taken(name)) return name;
  }
  return `${stem}${separator}1000.md`;
}

function copyBase(text: string, name: string): string {
  const title = noteTitle(text)
    .replace(/[\\/:*?"<>|]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  return title.slice(0, 60).trim() || name.replace(/\.md$/i, "");
}

function writable(): void {
  if (flag() === "locked") {
    throw new MockBackendError(
      "notesLocked",
      "these notes are open in another TrailMark window, so they can only be read here",
    );
  }
}

// ---- the commands --------------------------------------------------------------------------------------------

export const launchMode = (): LaunchMode => ({ notes: flag() !== null });
export const notesFolder = (): NotesFolder => folder();

export function setNotesFolder(path: string): NotesFolder {
  if (/missing/i.test(path)) {
    throw new MockBackendError("notFound", `not found: ${path}`, { subject: path });
  }
  notes.folderMissing = false;
  return { path, status: "ready", isDefault: false };
}

export function createNotesFolder(): NotesFolder {
  notes.folderMissing = false;
  if (flag() === "missing") notes.seeded = false;
  return { ...folder(), status: "ready" };
}

export const notesStartIn = (): StartIn => notes.startIn;
export function setNotesStartIn(startIn: StartIn): void {
  notes.startIn = startIn;
}

export function notesOverview(): NotesOverview {
  seed();
  const all = sortedEntries().filter((n) => n.kind === "note");
  const summary: NotesSummary = {
    pinned: all.filter((n) => n.pinned),
    latest: all.filter((n) => !n.pinned).slice(0, 3),
  };
  return { folder: folder(), summary };
}

export function openNotes(): NotesOpened {
  if (folder().status === "missing") {
    throw new MockBackendError("notFound", `not found: ${FOLDER}`, { subject: FOLDER });
  }
  seed();
  mockState.vault = "notes";
  const readOnly: NotesReadOnly | null = flag() === "locked" ? "locked" : null;
  const info: VaultInfo = {
    root: FOLDER,
    name: "TrailMark Notes",
    kind: "notes",
    mode: "ide",
    initialFile: null,
    worktrees: [],
    liveUpdates: true,
    treeDefault: null,
  };
  return { info, readOnly, recovered: [] };
}

export function listNotes(): NoteList {
  const all = sortedEntries();
  return { notes: all, open: all.some((n) => n.path === notes.open) ? notes.open : null };
}

export function newNote(): string {
  writable();
  const name = freeName(stamp(), "-");
  notes.reserved.add(name);
  return name;
}

export function forgetNewNote(path: string): void {
  notes.reserved.delete(path);
}

/** The editor's view of a new note that has no file yet; null for any other path. */
export function unbornView(path: string): DocumentView | null {
  if (!notes.reserved.has(path) || existing().includes(path)) return null;
  const view = markdownToView("");
  return {
    path,
    kind: "markdown",
    html: view.html,
    blocks: view.blocks,
    title: null,
    frontmatter: null,
    modifiedMs: Date.now(),
    sizeBytes: 0,
    encodingNote: null,
  };
}

/** The editor's source of a new note that has no file yet; null for any other path. */
export function unbornSource(path: string): SourceDoc | null {
  if (!notes.reserved.has(path) || existing().includes(path)) return null;
  return { path, text: "", hash: "", modifiedMs: Date.now(), ...mockParseBlocks("") };
}

function takeVersion(name: string, replaced: string, replacedAt: number, now: number): void {
  const last = notes.lastVersionAt.get(name);
  if (last !== undefined && now - last < VERSION_GAP_MS) return;
  notes.lastVersionAt.set(name, now);
  const list = notes.versions.get(name) ?? [];
  if (list[0]?.text === replaced) return;
  list.unshift({ id: `v${now}`, writtenMs: replacedAt, text: replaced });
  notes.versions.set(name, list);
}

export async function saveNote(
  path: string,
  text: string,
  expectedHash: string,
): Promise<NoteSaved> {
  writable();
  const now = Date.now();
  const create = (wanted: string): NoteSaved => {
    const name = existing().includes(wanted) ? freeName(wanted.replace(/\.md$/i, ""), "-") : wanted;
    putNote(name, text, now);
    notes.reserved.delete(path);
    notes.lastVersionAt.set(name, now);
    emitMock("tree-changed", null);
    return { result: "saved", path: name, hash: mockHash(text), modifiedMs: now, created: true };
  };
  if (expectedHash === "") return isBlank(text) ? { result: "blank" } : create(path);
  if (!existing().includes(path)) return create(path);
  const current = textOf(path);
  if (mockHash(current) !== expectedHash) {
    const name = freeName(`${copyBase(text, path)} (conflict ${stamp()})`, " ");
    putNote(name, text, now);
    emitMock("tree-changed", null);
    return { result: "conflict", copy: name };
  }
  if (current !== text) {
    takeVersion(path, current, sourceMtime(path), now);
    putNote(path, text, now);
    emitMock("tree-changed", null);
  }
  return {
    result: "saved",
    path,
    hash: mockHash(text),
    modifiedMs: sourceMtime(path),
    created: false,
  };
}

export function noteOpened(path: string): void {
  notes.open = path;
  notes.lastVersionAt.delete(path);
}

export function closeNote(): void {
  notes.open = null;
}

export function setNoteView(path: string, view: NoteView): void {
  notes.views.set(path, view);
}

export function setNotePinned(path: string, pinned: boolean): void {
  writable();
  notes.pins = notes.pins.filter((pin) => pin !== path);
  if (pinned) notes.pins.push(path);
}

export function setPinOrder(order: string[]): void {
  writable();
  const next = order.filter((name, at) => notes.pins.includes(name) && order.indexOf(name) === at);
  notes.pins = [...next, ...notes.pins.filter((name) => !next.includes(name))];
}

export function renameNote(path: string, name: string): string {
  writable();
  const typed = name.trim();
  if (typed === "" || /[\\/:*?"<>|]/.test(typed) || /[. ]$/.test(typed) || typed.startsWith(".")) {
    throw new MockBackendError("invalidName", "That name can't be used", {
      reason: typed === "" ? "empty" : "forbiddenChars",
    });
  }
  const to = /\.(md|markdown)$/i.test(typed) ? typed : `${typed}.md`;
  if (to === path) return to;
  if (existing().some((n) => n.toLowerCase() === to.toLowerCase() && n !== path)) {
    throw new MockBackendError("exists", `a note named ${to} already exists`, { subject: to });
  }
  const text = textOf(path);
  const mtime = sourceMtime(path);
  mockState.deleted.add(path);
  mockNoteFiles.delete(path);
  putNote(to, text, mtime);
  notes.pins = notes.pins.map((pin) => (pin === path ? to : pin));
  if (notes.open === path) notes.open = to;
  const view = notes.views.get(path);
  if (view) {
    notes.views.delete(path);
    notes.views.set(to, view);
  }
  const versions = notes.versions.get(path);
  if (versions) {
    notes.versions.delete(path);
    notes.versions.set(to, versions);
  }
  emitMock("tree-changed", null);
  return to;
}

export function deleteNotes(paths: string[]): NoteDeleted {
  writable();
  const batch = paths.map((name) => ({ name, text: textOf(name), mtime: sourceMtime(name) }));
  for (const name of paths) {
    mockState.deleted.add(name);
    mockNoteFiles.delete(name);
  }
  const undoId = notes.nextUndo++;
  notes.deleted.set(undoId, batch);
  emitMock("tree-changed", null);
  return { deleted: [...paths], undoId };
}

export function undoDeleteNotes(undoId: number): string[] {
  writable();
  const batch = notes.deleted.get(undoId);
  if (!batch) throw new MockBackendError("unknownUndo", "there is nothing to undo");
  notes.deleted.delete(undoId);
  const back = batch.map(({ name, text, mtime }) => {
    const target = existing().includes(name)
      ? freeName(`${name.replace(/\.md$/i, "")} (restored)`, " ")
      : name;
    putNote(target, text, mtime);
    return target;
  });
  emitMock("tree-changed", null);
  return back;
}

export function deleteCopiedNotes(): NoteDeleted {
  const copied = sortedEntries()
    .filter((n) => n.copied && !n.pinned)
    .map((n) => n.path);
  return copied.length === 0 ? { deleted: [], undoId: null } : deleteNotes(copied);
}

export function markNoteCopied(path: string): void {
  notes.copied.set(path, sourceMtime(path));
}

export function noteVersions(path: string): NoteVersion[] {
  return (notes.versions.get(path) ?? []).map((v) => ({
    id: v.id,
    writtenMs: v.writtenMs,
    sizeBytes: new TextEncoder().encode(v.text).length,
  }));
}

export function noteVersionText(path: string, id: string): string {
  const version = notes.versions.get(path)?.find((v) => v.id === id);
  if (!version) throw new MockBackendError("noSuchVersion", "that earlier version is not there");
  return version.text;
}

export function restoreNoteVersion(path: string, id: string): NoteRestored {
  writable();
  const text = noteVersionText(path, id);
  const now = Date.now();
  const current = textOf(path);
  const list = notes.versions.get(path) ?? [];
  list.unshift({ id: `v${now}`, writtenMs: sourceMtime(path), text: current });
  notes.versions.set(path, list);
  putNote(path, text, now);
  emitMock("tree-changed", null);
  return { hash: mockHash(text), modifiedMs: now };
}

export function discardBlankNote(path: string): boolean {
  writable();
  if (notes.pins.includes(path) || !existing().includes(path) || !isBlank(textOf(path))) {
    return false;
  }
  mockState.deleted.add(path);
  mockNoteFiles.delete(path);
  return true;
}

export function showNotes(
  newNoteWanted: boolean,
  open: string | null,
  openFile: string | null = null,
): void {
  console.info("[mock] show the Notes window:", { newNote: newNoteWanted, open, openFile });
  notes.request = { newNote: newNoteWanted, open, openFile };
  emitMock("notes-request", null);
}

// ---- loose files ---------------------------------------------------------------------------------------------

const baseName = (path: string): string => path.split(/[\\/]/).pop() ?? path;
const folderOf = (path: string): string => path.slice(0, path.length - baseName(path).length - 1);

/** Opens a file of the disk: what it is when it is one of the notes, else a loose file under Files. */
export function openLooseFile(path: string): OpenedFile {
  writable();
  seed();
  const name = baseName(path);
  if (/binary|\.(png|pdf|zip|exe)$/i.test(name)) {
    throw new MockBackendError("binaryFile", `${name} is not a text file`, { subject: name });
  }
  if (path.replace(/\\/g, "/").startsWith(`${FOLDER}/`) && existing().includes(name)) {
    return { kind: "note", path: name };
  }
  const known = notes.loose.find((file) => `${file.folder}\\${file.name}` === path);
  const now = Date.now();
  if (known) {
    known.openedMs = now;
    return { kind: "file", path: looseUi(known) };
  }
  const id = (0x3d4e5f60 + notes.loose.length * 0x11111111).toString(16).padStart(8, "0").slice(-8);
  const file: MockLoose = {
    id,
    name,
    folder: folderOf(path) || "~\\Downloads",
    openedMs: now,
    missing: false,
    changed: false,
  };
  notes.loose.push(file);
  const text = /\.txt$/i.test(name) ? "a plain text file\n" : `# ${name.replace(/\.\w+$/, "")}\n`;
  putNote(looseUi(file), text, now - DAY);
  return { kind: "file", path: looseUi(file) };
}

/** What the mock calls "reading" a loose file: it is the version seen from now on. */
export function seenLoose(path: string): void {
  const file = looseFile(path);
  if (file) file.changed = false;
}

export function looseStatus(): LooseStatus[] {
  return notes.loose.map((file) => {
    const path = looseUi(file);
    return {
      path,
      state: file.missing ? "missing" : file.changed ? "changed" : "same",
      stamp: file.missing ? "" : `${sourceMtime(path)}-${textOf(path).length}`,
      was: null,
    };
  });
}

export function removeLooseFile(path: string): void {
  writable();
  const file = looseFile(path);
  if (!file) throw new MockBackendError("notANote", `${path} is not a note`, { subject: path });
  notes.loose = notes.loose.filter((other) => other !== file);
  notes.pins = notes.pins.filter((pin) => pin !== path);
  notes.pending.delete(path);
  notes.views.delete(path);
  if (notes.open === path) notes.open = null;
}

export function locateLooseFile(path: string, newPath: string): OpenedFile {
  writable();
  const file = looseFile(path);
  if (!file) throw new MockBackendError("notANote", `${path} is not a note`, { subject: path });
  const was = looseUi(file);
  file.name = baseName(newPath);
  file.folder = folderOf(newPath);
  file.missing = false;
  file.changed = false;
  const now = Date.now();
  putNote(looseUi(file), textOf(was) || `# ${file.name}\n`, now - DAY);
  notes.pins = notes.pins.map((pin) => (pin === was ? looseUi(file) : pin));
  return { kind: "file", path: looseUi(file) };
}

export function savePending(path: string, text: string, baseHash: string): void {
  writable();
  notes.pending.set(path, { path, baseHash, text, savedMs: Date.now() });
}

export function loadPending(path: string): PendingEdits | null {
  return notes.pending.get(path) ?? null;
}

export function clearPending(path: string): void {
  notes.pending.delete(path);
}

/** A save of a loose file in the mock: the edits kept for it are written into it, so they go. */
export function savedLoose(path: string): void {
  notes.pending.delete(path);
  seenLoose(path);
}

/** "Save it again": a file that is gone is written where it was. */
export function recreateLoose(path: string, text: string): CreatedDoc {
  writable();
  const file = looseFile(path);
  if (!file) throw new MockBackendError("notANote", `${path} is not a note`, { subject: path });
  if (!file.missing) {
    throw new MockBackendError("exists", `a file named ${file.name} already exists`, {
      subject: file.name,
    });
  }
  const now = Date.now();
  file.missing = false;
  file.changed = false;
  putNote(looseUi(file), text, now);
  notes.pending.delete(path);
  return { path, hash: mockHash(text), modifiedMs: now };
}

export function keepEditsAsNote(path: string, text: string): string {
  writable();
  const file = looseFile(path);
  if (!file) throw new MockBackendError("notANote", `${path} is not a note`, { subject: path });
  const name = freeName(`${file.name.replace(/\.\w+$/, "")} (my edits)`, " ");
  putNote(name, text, Date.now());
  notes.pending.delete(path);
  emitMock("tree-changed", null);
  return name;
}

export function classifyPath(path: string): PathClass {
  if (/missing|not-there/i.test(path)) return { kind: "missing", inProject: null };
  return { kind: /\.\w+$/.test(baseName(path)) ? "file" : "folder", inProject: null };
}

/** The mock's "someone else saved the open loose file": it changes on disk. */
export function touchLoose(path: string): void {
  const file = looseFile(path);
  if (!file || file.missing) return;
  const ui = looseUi(file);
  putNote(ui, `${textOf(ui).replace(/\n*$/, "\n")}\nAdded by someone else.\n`, Date.now());
  file.changed = true;
}

/** The mock's "someone deleted the open loose file". */
export function removeOnDisk(path: string): void {
  const file = looseFile(path);
  if (!file) return;
  file.missing = true;
  mockState.deleted.add(looseUi(file));
  mockNoteFiles.delete(looseUi(file));
}

export function takeNotesRequest(): NotesRequest | null {
  const request = notes.request;
  notes.request = null;
  return request;
}
