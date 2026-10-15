// The Notes half of the browser mock: the same commands as `notes_commands.rs`, over a few notes kept in memory. The
// notes are stored in the mock documents' "written" layer (see `fixtures.ts`), so reading and editing them goes through the
// ordinary mock commands; this module adds what the Notes window needs on top: names, pins, versions, copies, deleting.
//
// `?notes` starts the browser mock as the Notes window; `?notes=missing` shows the folder that is not there, and
// `?notes=locked` shows the notes of a second window (read-only).

import { emitMock } from "../api";
import { isBlank, noteTitle } from "../noteText";
import type {
  DocumentView,
  LaunchMode,
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

const notes = {
  seeded: false,
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
    path: name,
    title: noteTitle(text),
    snippet,
    modifiedMs,
    sizeBytes: new TextEncoder().encode(text).length,
    pinned: notes.pins.includes(name),
    copied: notes.copied.get(name) === modifiedMs,
    view: notes.views.get(name) ?? null,
  };
}

function sortedEntries(): NoteEntry[] {
  const all = existing().map(entry);
  const rank = (n: NoteEntry): number => notes.pins.indexOf(n.path);
  return all.sort((a, b) => {
    const [x, y] = [rank(a), rank(b)];
    if (x >= 0 && y >= 0) return x - y;
    if (x >= 0) return -1;
    if (y >= 0) return 1;
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
  const all = sortedEntries();
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

export function showNotes(newNoteWanted: boolean, open: string | null): void {
  console.info("[mock] show the Notes window:", { newNote: newNoteWanted, open });
  notes.request = { newNote: newNoteWanted, open };
  emitMock("notes-request", null);
}

export function takeNotesRequest(): NotesRequest | null {
  const request = notes.request;
  notes.request = null;
  return request;
}
