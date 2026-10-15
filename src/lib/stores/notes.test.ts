// @vitest-environment jsdom
// The Notes window's store: starting, the new note that has no file yet, leaving a note, copying, deleting with Undo,
// and what is asked and what is not. The backend is replaced; nothing waits for time.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../bridge";
import type {
  DocumentView,
  NoteEntry,
  NoteList,
  NoteSaved,
  NotesOpened,
  NotesRequest,
  ParsedBlocks,
  SourceDoc,
} from "../types";

const fake = vi.hoisted(() => ({
  launchMode: vi.fn(),
  notesFolder: vi.fn(),
  setNotesFolder: vi.fn(),
  createNotesFolder: vi.fn(),
  openNotes: vi.fn(),
  listNotes: vi.fn(),
  loadSession: vi.fn(),
  listTree: vi.fn(),
  takeNotesRequest: vi.fn(),
  newNote: vi.fn(),
  forgetNewNote: vi.fn(),
  discardBlankNote: vi.fn(),
  noteOpened: vi.fn(),
  setNoteView: vi.fn(),
  saveNote: vi.fn(),
  readDocument: vi.fn(),
  documentSource: vi.fn(),
  parseBlocks: vi.fn(),
  fileMeta: vi.fn(),
  markNoteCopied: vi.fn(),
  deleteNotes: vi.fn(),
  undoDeleteNotes: vi.fn(),
  deleteCopiedNotes: vi.fn(),
  setNotePinned: vi.fn(),
  setPinOrder: vi.fn(),
  renameNote: vi.fn(),
  noteVersions: vi.fn(),
  noteVersionText: vi.fn(),
  restoreNoteVersion: vi.fn(),
  search: vi.fn(),
  openNotesFolder: vi.fn(),
}));
vi.mock("../api", () => ({
  api: {
    ...fake,
    metaIndex: async () => [],
    appDistribution: async () => "standard",
    on: async () => () => {},
  },
}));
const platform = vi.hoisted(() => ({
  setWindowTitle: vi.fn(async () => {}),
  askUnsaved: vi.fn(async () => "keep"),
  askYesNo: vi.fn(async (_text: string, _yes: string, _no: string) => true),
  copyText: vi.fn(async () => true),
  guardWindowClose: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));
vi.mock("../platform", () => platform);

import { bridge } from "../bridge";
import { noOutline } from "../editor/outlineStub";
import { mockParseBlocks } from "../mock/mockBlocks";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { notes } from "./notes.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { vault } from "./vault.svelte";

const NOW = 1_792_074_720_000;

const entry = (path: string, over: Partial<NoteEntry> = {}): NoteEntry => ({
  path,
  title: path.replace(/\.md$/, ""),
  snippet: "",
  modifiedMs: NOW,
  sizeBytes: 10,
  pinned: false,
  copied: false,
  view: { mode: "source", caret: null, scroll: null },
  ...over,
});

const view = (path: string): DocumentView => ({
  path,
  kind: "markdown",
  html: "",
  blocks: [],
  title: null,
  frontmatter: null,
  modifiedMs: 1,
  sizeBytes: 1,
  encodingNote: null,
});

const source = (path: string, text: string, hash: string): SourceDoc => ({
  path,
  text,
  hash,
  modifiedMs: 100,
  ...mockParseBlocks(text),
});

const opened = (read: NotesOpened["readOnly"] = null, recovered: string[] = []): NotesOpened => ({
  info: {
    root: "C:/Notes",
    name: "Notes",
    kind: "notes",
    mode: "ide",
    initialFile: null,
    worktrees: [],
    liveUpdates: true,
    treeDefault: null,
  },
  readOnly: read,
  recovered,
});

/** An editor that is only a string. */
function fakeEditor(initial: string): EditorApi & { text: string } {
  const editor = {
    ...noOutline,
    mode: "source" as const,
    text: initial,
    getText: () => editor.text,
    setText: (text: string) => {
      editor.text = text;
    },
    caretLine: () => 3,
    topLine: () => 1,
    focus: vi.fn(),
    insertImage: () => {},
    applyChanges: () => {},
  };
  return editor;
}

/** What the backend holds: the texts of the notes by name, with their hashes. */
let disk: Map<string, { text: string; hash: string }>;
let list: NoteEntry[];
let openName: string | null;
let request: NotesRequest | null;
let counter: number;

function backend(): void {
  fake.launchMode.mockResolvedValue({ notes: true });
  fake.notesFolder.mockResolvedValue({ path: "C:/Notes", status: "ready", isDefault: true });
  fake.openNotes.mockImplementation(async () => opened());
  fake.listNotes.mockImplementation(async (): Promise<NoteList> => ({
    notes: list,
    open: openName,
  }));
  fake.loadSession.mockResolvedValue(null);
  fake.listTree.mockResolvedValue({ name: "Notes", path: "", kind: "dir", children: [] });
  fake.takeNotesRequest.mockImplementation(async () => {
    const taken = request;
    request = null;
    return taken;
  });
  fake.newNote.mockImplementation(async () => `new-${++counter}.md`);
  fake.forgetNewNote.mockResolvedValue(undefined);
  fake.discardBlankNote.mockResolvedValue(false);
  fake.noteOpened.mockResolvedValue(undefined);
  fake.setNoteView.mockResolvedValue(undefined);
  fake.readDocument.mockImplementation(async (path: string) => view(path));
  fake.documentSource.mockImplementation(async (path: string) => {
    const file = disk.get(path);
    return source(path, file?.text ?? "", file?.hash ?? "");
  });
  fake.parseBlocks.mockImplementation(async (text: string): Promise<ParsedBlocks> =>
    mockParseBlocks(text),
  );
  fake.fileMeta.mockResolvedValue({ path: "", tags: [], description: "", writable: true });
  fake.markNoteCopied.mockResolvedValue(undefined);
  fake.search.mockResolvedValue([]);
  fake.saveNote.mockImplementation(async (path: string, text: string): Promise<NoteSaved> => {
    disk.set(path, { text, hash: `h${++counter}` });
    return {
      result: "saved",
      path,
      hash: disk.get(path)?.hash ?? "",
      modifiedMs: NOW,
      created: false,
    };
  });
}

function addNote(path: string, text: string, over: Partial<NoteEntry> = {}): void {
  disk.set(path, { text, hash: `h-${path}` });
  list = [...list, entry(path, over)];
}

async function start(): Promise<void> {
  await notes.start();
  // the editor of the open note is on screen
  if (edit.active) bridge.editor = fakeEditor(edit.initial?.buffer ?? "");
}

/** Types into the open note the way the Source editor reports it. */
function type(text: string): void {
  const editor = bridge.editor as ReturnType<typeof fakeEditor>;
  editor.text = text;
  edit.noteChange(text.length);
}

beforeEach(() => {
  vi.useFakeTimers();
  disk = new Map();
  list = [];
  openName = null;
  request = null;
  counter = 0;
  for (const mock of Object.values(fake)) mock.mockReset();
  for (const [key, mock] of Object.entries(platform)) {
    if (key !== "assetUrl" && key !== "isMock") (mock as ReturnType<typeof vi.fn>).mockClear();
  }
  platform.copyText.mockResolvedValue(true);
  platform.askYesNo.mockResolvedValue(true);
  backend();
  notes.reset();
  edit.reset();
  edit.autosave = true;
  docs.reset();
  docs.guard = (resume) => edit.holdNavigation(resume);
  bridge.editor = undefined;
  vault.reset();
  toast.clear();
  session.editMode = "source";
});
afterEach(() => {
  vi.useRealTimers();
});

describe("starting", () => {
  it("opens the note the window was left on, in the editor it was left in", async () => {
    addNote("a.md", "# A\n");
    addNote("b.md", "# B\n", { view: { mode: "source", caret: 1, scroll: 0 } });
    openName = "b.md";
    await start();
    expect(notes.phase).toBe("ready");
    expect(notes.current).toBe("b.md");
    expect(notes.unborn).toBe(false);
    expect(edit.active).toBe(true);
    expect(edit.path).toBe("b.md");
    expect(vault.isNotes).toBe(true);
    expect(fake.noteOpened).toHaveBeenCalledWith("b.md");
    expect(fake.newNote).not.toHaveBeenCalled();
  });

  it("opens a note in Read mode when that is how it was left", async () => {
    addNote("a.md", "# A\n", { view: { mode: "read", caret: null, scroll: 4 } });
    openName = "a.md";
    await start();
    expect(notes.current).toBe("a.md");
    expect(edit.active).toBe(false);
    expect(docs.path).toBe("a.md");
  });

  it("starts with a new note, ready to type in, when none was open", async () => {
    addNote("a.md", "# A\n");
    await start();
    expect(fake.newNote).toHaveBeenCalledTimes(1);
    expect(notes.current).toBe("new-1.md");
    expect(notes.unborn).toBe(true);
    expect(edit.active).toBe(true);
    expect(edit.mode).toBe("visual");
    // it has no file: its source is empty and has no hash
    expect(fake.documentSource).toHaveBeenCalledWith("new-1.md");
    expect(fake.saveNote).not.toHaveBeenCalled();
  });

  it("goes where another window asked: a new note, or a note", async () => {
    addNote("a.md", "# A\n");
    addNote("b.md", "# B\n");
    openName = "a.md";
    request = { newNote: false, open: "b.md" };
    await start();
    expect(notes.current).toBe("b.md");
    notes.reset();
    edit.reset();
    docs.reset();
    request = { newNote: true, open: null };
    await start();
    expect(notes.unborn).toBe(true);
  });

  it("asks nothing and makes nothing when the notes folder is not there", async () => {
    fake.notesFolder.mockResolvedValue({ path: "D:/Notes", status: "missing", isDefault: true });
    await start();
    expect(notes.phase).toBe("missing");
    expect(fake.openNotes).not.toHaveBeenCalled();
    expect(fake.createNotesFolder).not.toHaveBeenCalled();
    expect(fake.newNote).not.toHaveBeenCalled();
  });

  it("opens the folder the person located, and makes the one that was missing only when asked", async () => {
    fake.notesFolder.mockResolvedValue({ path: "D:/Notes", status: "missing", isDefault: true });
    await start();
    fake.setNotesFolder.mockResolvedValue({ path: "E:/Mine", status: "ready", isDefault: false });
    await notes.locate("E:/Mine");
    expect(fake.setNotesFolder).toHaveBeenCalledWith("E:/Mine");
    expect(notes.phase).toBe("ready");
    expect(fake.createNotesFolder).not.toHaveBeenCalled();

    notes.reset();
    fake.notesFolder.mockResolvedValue({ path: "D:/Notes", status: "missing", isDefault: true });
    await start();
    fake.createNotesFolder.mockResolvedValue({
      path: "D:/Notes",
      status: "ready",
      isDefault: true,
    });
    await notes.createFolder();
    expect(fake.createNotesFolder).toHaveBeenCalledTimes(1);
    expect(notes.phase).toBe("ready");
  });

  it("says when notes were recovered, and opens the notes of a second window to read only", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    fake.openNotes.mockResolvedValue(opened("locked", ["A (recovered).md"]));
    await start();
    expect(notes.readOnly).toBe("locked");
    expect(edit.active).toBe(false);
    expect(edit.blocked).toMatch(/another TrailMark window/);
    expect(toast.current).toMatchObject({ type: "message" });
    // nothing is created in a window that cannot write
    expect(fake.newNote).not.toHaveBeenCalled();
  });

  it("shows the reason when the notes cannot be opened", async () => {
    fake.openNotes.mockRejectedValue({ kind: "notFound", message: "gone", subject: "C:/Notes" });
    await start();
    expect(notes.phase).toBe("failed");
    expect(notes.failure).not.toBeNull();
  });
});

describe("leaving a note", () => {
  it("forgets a new note that was never written, and removes a blank note nobody wrote in", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    // a new note with nothing in it
    await notes.newNote();
    const fresh = notes.current ?? "";
    expect(fresh).toBe("new-1.md");
    await notes.openNote("a.md");
    expect(fake.forgetNewNote).toHaveBeenCalledWith(fresh);
    expect(fake.discardBlankNote).not.toHaveBeenCalled();
    expect(fake.saveNote).not.toHaveBeenCalled();
    expect(notes.current).toBe("a.md");
  });

  it("removes a note that was blank when it was opened and still is", async () => {
    addNote("blank.md", "  \n", { title: "" });
    addNote("a.md", "# A\n");
    openName = "blank.md";
    fake.discardBlankNote.mockResolvedValue(true);
    await start();
    expect(notes.current).toBe("blank.md");
    await notes.openNote("a.md");
    expect(fake.discardBlankNote).toHaveBeenCalledWith("blank.md");
  });

  it("keeps a note the person emptied: it was written in", async () => {
    addNote("a.md", "# A\n");
    addNote("b.md", "# B\n");
    openName = "a.md";
    await start();
    type("");
    await notes.openNote("b.md");
    expect(fake.discardBlankNote).not.toHaveBeenCalled();
    // and what was cleared was saved first
    expect(fake.saveNote).toHaveBeenCalledWith("a.md", "", "h-a.md");
  });

  it("saves the note before moving to another, and remembers where it was left", async () => {
    addNote("a.md", "# A\n");
    addNote("b.md", "# B\n");
    openName = "a.md";
    await start();
    type("# A\n\nchanged");
    await notes.openNote("b.md");
    expect(fake.saveNote).toHaveBeenCalledWith("a.md", "# A\n\nchanged", "h-a.md");
    expect(fake.setNoteView).toHaveBeenCalledWith("a.md", { mode: "source", caret: 3, scroll: 1 });
    expect(notes.current).toBe("b.md");
    expect(platform.askUnsaved).not.toHaveBeenCalled();
  });

  it("does not leave a note that cannot be saved", async () => {
    addNote("a.md", "# A\n");
    addNote("b.md", "# B\n");
    openName = "a.md";
    await start();
    type("# A\n\nchanged");
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: "a.md" });
    expect(await notes.openNote("b.md")).toBe(false);
    expect(notes.current).toBe("a.md");
    expect(edit.dirty).toBe(true);
  });

  it("keeps the note being typed in where it is in the list until it is left", async () => {
    addNote("a.md", "# A\n", { modifiedMs: NOW - 90 * 24 * 3600 * 1000 });
    addNote("b.md", "# B\n", { modifiedMs: NOW - 3600 * 1000 });
    openName = "a.md";
    await start();
    const before = notes.order(NOW);
    expect(before).toEqual(["b.md", "a.md"]);
    // typing saves it, and the list is told it is newer: it stays where it was
    type("# A\nnew");
    await edit.flush();
    list = list.map((n) => (n.path === "a.md" ? { ...n, modifiedMs: NOW } : n));
    await notes.refresh();
    expect(notes.order(NOW)).toEqual(before);
    // left, it moves up
    await notes.openNote("b.md");
    expect(notes.order(NOW)[0]).toBe("a.md");
  });
});

describe("a new note", () => {
  it("is shown at the top with no title until it is written, and gets its file with the first text", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    await notes.newNote();
    expect(notes.shown[0]).toMatchObject({ path: "new-1.md", title: "" });
    fake.saveNote.mockResolvedValue({
      result: "saved",
      path: "new-1.md",
      hash: "n1",
      modifiedMs: NOW,
      created: true,
    } satisfies NoteSaved);
    list = [entry("new-1.md", { title: "Hello" }), ...list];
    type("Hello");
    await edit.flush();
    expect(fake.saveNote).toHaveBeenCalledWith("new-1.md", "Hello", "");
    expect(notes.unborn).toBe(false);
    expect(fake.noteOpened).toHaveBeenLastCalledWith("new-1.md");
    await vi.advanceTimersByTimeAsync(1000);
    expect(notes.entries.some((n) => n.path === "new-1.md")).toBe(true);
  });

  it("does not make a second new note on top of one that is still empty", async () => {
    await start();
    expect(fake.newNote).toHaveBeenCalledTimes(1);
    await notes.newNote();
    expect(fake.newNote).toHaveBeenCalledTimes(1);
  });

  it("is refused in a window that can only read", async () => {
    fake.openNotes.mockResolvedValue(opened("locked"));
    await start();
    await notes.newNote();
    expect(fake.newNote).not.toHaveBeenCalled();
  });
});

describe("copying", () => {
  it("copies the text of the note exactly as it is, marks it copied, and offers to delete it", async () => {
    const text = "\uFEFF# Prompt\r\n\r\nAsk the agent to add tests.\r\n\r\n- keep the glow\r\n";
    addNote("a.md", text);
    openName = "a.md";
    await start();
    bridge.editor = {
      ...(bridge.editor as EditorApi),
      getText: () => "# Prompt\n\nAsk the agent to add tests.\n\n- keep the glow\n",
    } as EditorApi;
    await notes.copyNote();
    expect(platform.copyText).toHaveBeenCalledTimes(1);
    // the file's own text: the BOM and the line endings are kept
    expect(platform.copyText).toHaveBeenCalledWith(text);
    expect(fake.markNoteCopied).toHaveBeenCalledWith("a.md");
    expect(notes.entries.find((n) => n.path === "a.md")?.copied).toBe(true);
    expect(toast.current).toMatchObject({
      type: "message",
      text: `Copied ${Array.from(text).length} characters`,
      action: { label: "Delete note" },
    });
  });

  it("copies the unsaved text, saved first", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    type("# A\n\nand more");
    await notes.copyNote();
    expect(fake.saveNote).toHaveBeenCalledWith("a.md", "# A\n\nand more", "h-a.md");
    expect(platform.copyText).toHaveBeenCalledWith("# A\n\nand more");
  });

  it("does not offer to delete a pinned note, and says when there is nothing to copy", async () => {
    addNote("a.md", "# A\n", { pinned: true });
    openName = "a.md";
    await start();
    await notes.copyNote();
    expect(toast.current).toMatchObject({ type: "message" });
    expect(
      toast.current && "action" in toast.current ? toast.current.action : undefined,
    ).toBeUndefined();
    toast.clear();
    notes.reset();
    edit.reset();
    docs.reset();
    list = [];
    await start();
    await notes.copyNote();
    expect(platform.copyText).toHaveBeenCalledTimes(1);
    expect(toast.current).toMatchObject({ text: "There is nothing to copy yet." });
  });

  it("deletes the note from the toast's button", async () => {
    addNote("a.md", "# A\n");
    addNote("b.md", "# B\n");
    openName = "a.md";
    await start();
    await notes.copyNote();
    fake.deleteNotes.mockResolvedValue({ deleted: ["a.md"], undoId: 5 });
    list = list.filter((n) => n.path !== "a.md");
    toast.runAction();
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.deleteNotes).toHaveBeenCalledWith(["a.md"]);
  });
});

describe("deleting", () => {
  beforeEach(() => {
    addNote("a.md", "# A\n");
    addNote("b.md", "# B\n");
    addNote("sticky.md", "# Sticky\n", { pinned: true });
    openName = "a.md";
  });

  it("sends the note to the bin, shows Undo, and moves on to a neighbour", async () => {
    await start();
    fake.deleteNotes.mockResolvedValue({ deleted: ["a.md"], undoId: 7 });
    list = list.filter((n) => n.path !== "a.md");
    await notes.deleteNotes(["a.md"]);
    expect(fake.deleteNotes).toHaveBeenCalledWith(["a.md"]);
    expect(platform.askYesNo).not.toHaveBeenCalled();
    expect(toast.current).toMatchObject({ text: "Deleted “a”", action: { label: "Undo" } });
    // nothing writes the deleted note back
    await vi.advanceTimersByTimeAsync(10_000);
    expect(fake.saveNote).not.toHaveBeenCalled();
    expect(notes.current).not.toBe("a.md");
    expect(notes.current).not.toBeNull();
  });

  it("brings the note back with Undo, and opens it", async () => {
    await start();
    fake.deleteNotes.mockResolvedValue({ deleted: ["a.md"], undoId: 7 });
    list = list.filter((n) => n.path !== "a.md");
    await notes.deleteNotes(["a.md"]);
    fake.undoDeleteNotes.mockImplementation(async () => {
      list = [...list, entry("a.md")];
      return ["a.md"];
    });
    toast.runAction();
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.undoDeleteNotes).toHaveBeenCalledWith(7);
    expect(notes.current).toBe("a.md");
  });

  it("asks before deleting a pinned note, and does nothing when the answer is no", async () => {
    await start();
    platform.askYesNo.mockResolvedValue(false);
    await notes.deleteNotes(["sticky.md"]);
    expect(platform.askYesNo).toHaveBeenCalledTimes(1);
    expect(platform.askYesNo.mock.calls[0]?.[0]).toBe("“sticky” is pinned. Delete it anyway?");
    expect(fake.deleteNotes).not.toHaveBeenCalled();
    platform.askYesNo.mockResolvedValue(true);
    fake.deleteNotes.mockResolvedValue({ deleted: ["sticky.md"], undoId: 8 });
    await notes.deleteNotes(["sticky.md"]);
    expect(fake.deleteNotes).toHaveBeenCalledWith(["sticky.md"]);
  });

  it("deletes the copied notes but not the pinned ones, with Undo", async () => {
    list = list.map((n) =>
      n.path === "b.md" || n.path === "sticky.md" ? { ...n, copied: true } : n,
    );
    await start();
    expect(notes.copiedCount).toBe(1);
    fake.deleteCopiedNotes.mockResolvedValue({ deleted: ["b.md"], undoId: 9 });
    list = list.filter((n) => n.path !== "b.md");
    await notes.deleteCopied();
    expect(fake.deleteCopiedNotes).toHaveBeenCalledTimes(1);
    expect(toast.current).toMatchObject({
      text: "Deleted 1 copied note",
      action: { label: "Undo" },
    });
    expect(notes.entries.some((n) => n.path === "sticky.md")).toBe(true);
  });

  it("does nothing in a window that can only read", async () => {
    fake.openNotes.mockResolvedValue(opened("locked"));
    await start();
    await notes.deleteNotes(["a.md"]);
    await notes.deleteCopied();
    expect(fake.deleteNotes).not.toHaveBeenCalled();
    expect(fake.deleteCopiedNotes).not.toHaveBeenCalled();
  });
});

describe("pinning and renaming", () => {
  beforeEach(() => {
    addNote("a.md", "# A\n", { pinned: true });
    addNote("b.md", "# B\n", { pinned: true });
    addNote("c.md", "# C\n");
    openName = "c.md";
  });

  it("moves a pinned note one place, or drops it on another", async () => {
    await start();
    await notes.movePin("b.md", -1);
    expect(fake.setPinOrder).toHaveBeenLastCalledWith(["b.md", "a.md"]);
    await notes.dropPin("a.md", "b.md");
    expect(fake.setPinOrder).toHaveBeenLastCalledWith(["b.md", "a.md"]);
    await notes.movePin("a.md", -1);
    expect(fake.setPinOrder).toHaveBeenLastCalledWith(["a.md", "b.md"]);
  });

  it("jumps to the pinned notes in order", async () => {
    await start();
    await notes.openPinned(2);
    expect(notes.current).toBe("b.md");
    await notes.openPinned(9);
    expect(notes.current).toBe("b.md");
  });

  it("renames the open note: the file name changes and the note stays open", async () => {
    await start();
    fake.renameNote.mockImplementation(async () => {
      disk.set("Launch plan.md", disk.get("c.md") ?? { text: "", hash: "" });
      disk.delete("c.md");
      list = list.map((n) => (n.path === "c.md" ? { ...n, path: "Launch plan.md" } : n));
      return "Launch plan.md";
    });
    expect(await notes.rename("c.md", "Launch plan")).toBeNull();
    expect(fake.renameNote).toHaveBeenCalledWith("c.md", "Launch plan");
    expect(notes.current).toBe("Launch plan.md");
    expect(edit.path).toBe("Launch plan.md");
  });

  it("says why a name was refused", async () => {
    await start();
    fake.renameNote.mockRejectedValue({ kind: "exists", message: "x", subject: "a.md" });
    expect(await notes.rename("c.md", "a")).toBe("A note named a already exists.");
    expect(await notes.rename("c.md", "   ")).toBe("Type a name for the file.");
    expect(fake.renameNote).toHaveBeenCalledTimes(1);
  });
});

describe("earlier versions", () => {
  it("lists them, shows the one selected, and restores it", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    fake.noteVersions.mockResolvedValue([
      { id: "20261015-143200-123", writtenMs: NOW, sizeBytes: 20 },
      { id: "20261015-140000-000", writtenMs: NOW - 3_000_000, sizeBytes: 10 },
    ]);
    fake.noteVersionText.mockImplementation(async (_path: string, id: string) =>
      id === "20261015-143200-123" ? "# A\nnewer" : "# A\nolder",
    );
    await notes.showVersions();
    expect(notes.versions?.items).toHaveLength(2);
    expect(notes.versions?.selected).toBe("20261015-143200-123");
    expect(notes.versions?.text).toBe("# A\nnewer");
    await notes.selectVersion("20261015-140000-000");
    expect(notes.versions?.text).toBe("# A\nolder");
    fake.restoreNoteVersion.mockResolvedValue({ hash: "r1", modifiedMs: NOW });
    fake.documentSource.mockResolvedValue(source("a.md", "# A\nolder", "r1"));
    await notes.restoreVersion();
    expect(fake.restoreNoteVersion).toHaveBeenCalledWith("a.md", "20261015-140000-000");
    expect(notes.versions).toBeNull();
    expect((bridge.editor as ReturnType<typeof fakeEditor>).text).toBe("# A\nolder");
  });
});

describe("the filter", () => {
  it("filters by title and snippet at once, and by text after a pause", async () => {
    addNote("a.md", "# Server IPs\n", { title: "Server IPs", snippet: "api 10.0.0.4" });
    addNote("b.md", "# Weekly\n", { title: "Weekly checklist", snippet: "backups" });
    openName = "a.md";
    await start();
    notes.setFilter("weekly");
    expect(notes.shown.map((n) => n.path)).toEqual(["b.md"]);
    fake.search.mockResolvedValue([{ path: "a.md" }]);
    notes.setFilter("zzz");
    expect(notes.shown).toEqual([]);
    await vi.advanceTimersByTimeAsync(300);
    expect(fake.search).toHaveBeenCalledWith("zzz", 60);
    expect(notes.shown.map((n) => n.path)).toEqual(["a.md"]);
    notes.clearFilter();
    expect(notes.shown).toHaveLength(2);
  });
});

describe("what belongs to a project is off", () => {
  it("has no git, no unread markers and no agent in the notes window", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    expect(vault.isNotes).toBe(true);
    expect(vault.git).toBeNull();
    expect(vault.since).toBeNull();
    for (const name of ["gitSnapshot", "sinceLastVisit", "getUnread", "agentStatus"]) {
      expect(Object.keys(fake)).not.toContain(name);
    }
  });
});
