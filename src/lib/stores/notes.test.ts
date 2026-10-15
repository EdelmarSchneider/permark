// @vitest-environment jsdom
// The Notes window's store: starting, the new note that has no file yet, leaving a note, copying, deleting with Undo,
// and what is asked and what is not. The backend is replaced; nothing waits for time.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../bridge";
import type {
  DocumentView,
  LooseStatus,
  NoteEntry,
  NoteList,
  NoteSaved,
  NotesOpened,
  NotesRequest,
  ParsedBlocks,
  SaveOutcome,
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
  openLooseFile: vi.fn(),
  looseStatus: vi.fn(),
  removeLooseFile: vi.fn(),
  locateLooseFile: vi.fn(),
  loadPending: vi.fn(),
  savePending: vi.fn(),
  clearPending: vi.fn(),
  keepEditsAsNote: vi.fn(),
  saveDocument: vi.fn(),
  createDocument: vi.fn(),
  revealInFolder: vi.fn(),
  setNoteShared: vi.fn(),
  moveSuggestion: vi.fn(),
  moveNoteToProject: vi.fn(),
  undoMoveNote: vi.fn(),
  openInProject: vi.fn(),
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
  watchFileDrop: vi.fn(async () => () => {}),
  pickFile: vi.fn(async (): Promise<string | null> => null),
  pickSavePath: vi.fn(async (_default: string): Promise<string | null> => null),
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
  kind: "note",
  path,
  title: path.replace(/\.md$/, ""),
  snippet: "",
  modifiedMs: NOW,
  sizeBytes: 10,
  pinned: false,
  copied: false,
  view: { mode: "source", caret: null, scroll: null },
  folder: null,
  missing: false,
  unsaved: false,
  shared: false,
  ...over,
});

const view = (path: string): DocumentView => ({
  path,
  kind: /\.txt$/i.test(path) ? "text" : "markdown",
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
  fake.loadPending.mockResolvedValue(null);
  fake.savePending.mockResolvedValue(undefined);
  fake.clearPending.mockResolvedValue(undefined);
  fake.looseStatus.mockResolvedValue([]);
  fake.removeLooseFile.mockResolvedValue(undefined);
  fake.revealInFolder.mockResolvedValue(undefined);
  fake.saveDocument.mockImplementation(async (path: string, text: string): Promise<SaveOutcome> => {
    disk.set(path, { text, hash: `h${++counter}` });
    return { result: "saved", hash: disk.get(path)?.hash ?? "", modifiedMs: NOW };
  });
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

const FILE = "@file/0a1b2c3d/proposal.md";
const OTHER = "@file/1b2c3d4e/todo.txt";

/** A loose file: a file of the disk that is listed under Files. It opens in Read. */
function addFile(path: string, text: string, over: Partial<NoteEntry> = {}): void {
  const name = path.slice(path.lastIndexOf("/") + 1);
  disk.set(path, { text, hash: `h-${path}` });
  list = [
    ...list,
    entry(path, {
      kind: "file",
      title: name,
      folder: "~\\Downloads",
      view: null,
      ...over,
    }),
  ];
}

/** What the backend says of the loose files. */
function looseState(...statuses: Partial<LooseStatus>[]): void {
  fake.looseStatus.mockResolvedValue(
    statuses.map((s) => ({ path: FILE, state: "same", stamp: "10-1", was: null, ...s })),
  );
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

  it("counts the words of a note when it opens, in either mode", async () => {
    addNote("a.md", "# A title\n\n- [ ] one thing\n", {
      view: { mode: "read", caret: null, scroll: 0 },
    });
    addNote("b.md", "two words\n", { view: { mode: "source", caret: 1, scroll: 0 } });
    openName = "a.md";
    await start();
    expect(notes.words).toBe(4);
    await notes.openNote("b.md");
    expect(notes.words).toBe(2);
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
    request = { newNote: false, open: "b.md", openFile: null };
    await start();
    expect(notes.current).toBe("b.md");
    notes.reset();
    edit.reset();
    docs.reset();
    request = { newNote: true, open: null, openFile: null };
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

  it("with only blank text typed in can be left for another note", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    await notes.newNote();
    const fresh = notes.current ?? "";
    fake.saveNote.mockResolvedValue({ result: "blank" } satisfies NoteSaved);
    type("  \n");
    await notes.openNote("a.md");
    expect(fake.saveNote).toHaveBeenCalledWith(fresh, "  \n", "");
    expect(notes.current).toBe("a.md");
    expect(fake.forgetNewNote).toHaveBeenCalledWith(fresh);
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

  it("saves the open note before it goes to the bin, so Undo brings back what was typed", async () => {
    await start();
    fake.deleteNotes.mockResolvedValue({ deleted: ["a.md"], undoId: 7 });
    type("# A\ntyped last");
    list = list.filter((n) => n.path !== "a.md");
    await notes.deleteNotes(["a.md"]);
    expect(fake.saveNote).toHaveBeenCalledWith("a.md", "# A\ntyped last", "h-a.md");
    expect(fake.saveNote.mock.invocationCallOrder[0]).toBeLessThan(
      fake.deleteNotes.mock.invocationCallOrder[0],
    );
  });

  it("does not delete the open note when what was typed cannot be saved", async () => {
    await start();
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: "a.md" });
    type("# A\ntyped last");
    await notes.deleteNotes(["a.md"]);
    expect(fake.deleteNotes).not.toHaveBeenCalled();
    expect(notes.current).toBe("a.md");
    expect(edit.active).toBe(true);
    expect(toast.current).toMatchObject({ tone: "warn" });
  });

  it("saves the open copied note first and then keeps it, because it was edited after it was copied", async () => {
    list = list.map((n) => ({ ...n, copied: n.path !== "sticky.md" }));
    await start();
    expect(notes.copiedCount).toBe(2);
    fake.deleteCopiedNotes.mockResolvedValue({ deleted: ["b.md"], undoId: 9 });
    type("# A\nedited after the copy");
    list = list.filter((n) => n.path !== "b.md");
    await notes.deleteCopied();
    expect(fake.saveNote).toHaveBeenCalledWith("a.md", "# A\nedited after the copy", "h-a.md");
    expect(fake.saveNote.mock.invocationCallOrder[0]).toBeLessThan(
      fake.deleteCopiedNotes.mock.invocationCallOrder[0],
    );
    // the note being typed in is no longer a copied one: it stays open
    expect(notes.current).toBe("a.md");
    expect(edit.active).toBe(true);
  });

  it("does not delete the copied notes when the open one cannot be saved", async () => {
    list = list.map((n) => ({ ...n, copied: n.path !== "sticky.md" }));
    await start();
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: "a.md" });
    type("# A\nedited after the copy");
    await notes.deleteCopied();
    expect(fake.deleteCopiedNotes).not.toHaveBeenCalled();
    expect(notes.current).toBe("a.md");
    expect(toast.current).toMatchObject({ tone: "warn" });
  });

  it("opens a neighbour, not a new note, when the open note is deleted elsewhere", async () => {
    await start();
    expect(notes.current).toBe("a.md");
    // the file is gone from the folder
    disk.delete("a.md");
    list = list.filter((n) => n.path !== "a.md");
    edit.noteHooks.gone?.("a.md");
    await vi.advanceTimersByTimeAsync(0);
    expect(notes.current).toBe("b.md");
    expect(fake.newNote).not.toHaveBeenCalled();
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

describe("earlier versions of a note with text that cannot be saved", () => {
  beforeEach(async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    fake.noteVersions.mockResolvedValue([
      { id: "20261015-143200-123", writtenMs: NOW, sizeBytes: 20 },
    ]);
    fake.noteVersionText.mockResolvedValue("# A\nolder");
  });

  it("does not put a version back over text that was not saved", async () => {
    await notes.showVersions();
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: "a.md" });
    type("# A\ntyped and not saved");
    await notes.restoreVersion();
    expect(fake.restoreNoteVersion).not.toHaveBeenCalled();
    expect(notes.versions?.failed).toBe(
      "The note couldn't be saved first, so an earlier version can't be put back yet.",
    );
    expect((bridge.editor as ReturnType<typeof fakeEditor>).text).toBe("# A\ntyped and not saved");
    expect(edit.dirty).toBe(true);
  });

  it("does not open the list while the note cannot be saved", async () => {
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: "a.md" });
    type("# A\ntyped and not saved");
    await notes.showVersions();
    expect(notes.versions).toBeNull();
    expect(fake.noteVersions).not.toHaveBeenCalled();
    expect(toast.current).toMatchObject({ tone: "warn" });
  });
});

describe("sharing a note with agents", () => {
  beforeEach(() => {
    addNote("a.md", "# A\n");
    addNote("b.md", "# B\n");
    openName = "a.md";
    fake.setNoteShared.mockImplementation(async (path: string, shared: boolean) => {
      list = list.map((n) => (n.path === path ? { ...n, shared } : n));
    });
  });

  it("marks a note as shared and stops sharing it from the same place", async () => {
    await start();
    await notes.toggleShare("b.md");
    expect(fake.setNoteShared).toHaveBeenLastCalledWith("b.md", true);
    expect(notes.entries.find((n) => n.path === "b.md")?.shared).toBe(true);
    expect(toast.current).toMatchObject({ text: "“b” is shared with agents." });
    await notes.toggleShare("b.md");
    expect(fake.setNoteShared).toHaveBeenLastCalledWith("b.md", false);
    expect(notes.entries.find((n) => n.path === "b.md")?.shared).toBe(false);
    expect(toast.current).toMatchObject({ text: "“b” is not shared any more." });
  });

  it("works on the open note when none is given, and not on a new note, a file or a window that only reads", async () => {
    await start();
    await notes.toggleShare();
    expect(fake.setNoteShared).toHaveBeenCalledWith("a.md", true);
    fake.setNoteShared.mockClear();
    await notes.newNote();
    await notes.toggleShare();
    expect(fake.setNoteShared).not.toHaveBeenCalled();
    fake.openNotes.mockResolvedValue(opened("locked"));
    notes.reset();
    await start();
    await notes.toggleShare("b.md");
    expect(fake.setNoteShared).not.toHaveBeenCalled();
  });

  it("says why when it cannot be done", async () => {
    await start();
    fake.setNoteShared.mockRejectedValue({ kind: "notANote", message: "no", subject: "b.md" });
    await notes.toggleShare("b.md");
    expect(toast.current).toMatchObject({ tone: "warn" });
    expect(notes.entries.find((n) => n.path === "b.md")?.shared).toBe(false);
  });
});

describe("moving a note into a project", () => {
  const COPY = "@file/0a1b2c3d/launch-plan.md";

  beforeEach(() => {
    addNote("a.md", "# Launch plan\n", { title: "Launch plan" });
    addNote("b.md", "# B\n");
    openName = "a.md";
    fake.moveSuggestion.mockResolvedValue({
      defaultPath: "C:/work/plan/Launch plan.md",
      folder: "C:/work/plan",
    });
    platform.pickSavePath.mockResolvedValue("C:/work/plan/launch-plan.md");
    fake.moveNoteToProject.mockImplementation(async (path: string, destination: string) => {
      list = [
        ...list.filter((n) => n.path !== path),
        entry(COPY, { kind: "file", title: "launch-plan.md", folder: "C:\\work\\plan" }),
      ];
      return { destination, file: COPY, undoId: 5 };
    });
  });

  it("saves the note, asks where, writes the copy, and offers Open in project and Undo", async () => {
    await start();
    type("# Launch plan\ntyped last");
    await notes.moveToProject();
    expect(fake.saveNote).toHaveBeenCalledWith("a.md", "# Launch plan\ntyped last", "h-a.md");
    expect(platform.pickSavePath).toHaveBeenCalledWith("C:/work/plan/Launch plan.md");
    expect(fake.moveNoteToProject).toHaveBeenCalledWith("a.md", "C:/work/plan/launch-plan.md");
    expect(fake.saveNote.mock.invocationCallOrder[0]).toBeLessThan(
      fake.moveNoteToProject.mock.invocationCallOrder[0],
    );
    expect(toast.current).toMatchObject({
      text: "Moved “Launch plan” to plan",
      action: { label: "Open in project" },
      second: { label: "Undo" },
    });
    // the note is gone from the list and the next one is open; the copy is a row among the files
    expect(notes.current).toBe("b.md");
    expect(notes.entries.some((n) => n.path === "a.md")).toBe(false);
    expect(notes.entries.some((n) => n.path === COPY)).toBe(true);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(fake.saveNote).toHaveBeenCalledTimes(1);
  });

  it("moves a note that is not the open one, without touching the open one", async () => {
    await start();
    await notes.moveToProject("b.md");
    expect(fake.moveNoteToProject).toHaveBeenCalledWith("b.md", "C:/work/plan/launch-plan.md");
    expect(notes.current).toBe("a.md");
    expect(edit.active).toBe(true);
  });

  it("moves nothing when the dialog is cancelled", async () => {
    await start();
    platform.pickSavePath.mockResolvedValue(null);
    await notes.moveToProject();
    expect(fake.moveNoteToProject).not.toHaveBeenCalled();
    expect(notes.current).toBe("a.md");
    expect(edit.active).toBe(true);
  });

  it("keeps the note open and says so when a file of that name is there", async () => {
    await start();
    fake.moveNoteToProject.mockRejectedValue({
      kind: "exists",
      message: "exists",
      subject: "launch-plan.md",
    });
    await notes.moveToProject();
    expect(toast.current).toMatchObject({
      text: "There is a file named launch-plan.md there already. Nothing was moved.",
    });
    expect(notes.current).toBe("a.md");
    expect(edit.active).toBe(true);
    expect(notes.entries.some((n) => n.path === "a.md")).toBe(true);
  });

  it("does not move a note whose text could not be saved", async () => {
    await start();
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: "a.md" });
    type("# Launch plan\ntyped last");
    await notes.moveToProject();
    expect(platform.pickSavePath).not.toHaveBeenCalled();
    expect(fake.moveNoteToProject).not.toHaveBeenCalled();
    expect(toast.current).toMatchObject({ tone: "warn" });
    expect(notes.current).toBe("a.md");
  });

  it("opens the project on the copy from the toast", async () => {
    await start();
    await notes.moveToProject();
    toast.runAction();
    expect(fake.openInProject).toHaveBeenCalledWith("C:/work/plan/launch-plan.md");
  });

  it("brings the note back with Undo, and opens it", async () => {
    await start();
    await notes.moveToProject();
    fake.undoMoveNote.mockImplementation(async () => {
      list = [...list.filter((n) => n.path !== COPY), entry("a.md", { title: "Launch plan" })];
      return { note: "a.md", removed: true };
    });
    toast.runAction("second");
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.undoMoveNote).toHaveBeenCalledWith(5);
    expect(notes.current).toBe("a.md");
    expect(toast.current).toMatchObject({ text: "“Launch plan” is back in your notes." });
  });

  it("says when the copy was left in the project because it was changed", async () => {
    await start();
    await notes.moveToProject();
    fake.undoMoveNote.mockImplementation(async () => {
      list = [...list, entry("a.md", { title: "Launch plan" })];
      return { note: "a.md", removed: false };
    });
    toast.runAction("second");
    await vi.advanceTimersByTimeAsync(0);
    expect(toast.current).toMatchObject({
      text: "“Launch plan” is back in your notes. The copy was changed, so it stays in the project.",
    });
  });

  it("does nothing for a new note, a loose file, or a window that can only read", async () => {
    await start();
    await notes.newNote();
    await notes.moveToProject();
    expect(platform.pickSavePath).not.toHaveBeenCalled();
    expect(notes.movable).toBe(false);
    addFile("@file/0a1b2c3d/x.md", "# X\n");
    await notes.refresh();
    expect(notes.entries.some((n) => n.path === "@file/0a1b2c3d/x.md")).toBe(true);
    await notes.moveToProject("@file/0a1b2c3d/x.md");
    expect(platform.pickSavePath).not.toHaveBeenCalled();
    fake.openNotes.mockResolvedValue(opened("locked"));
    notes.reset();
    await start();
    await notes.moveToProject("b.md");
    expect(platform.pickSavePath).not.toHaveBeenCalled();
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

// ---- loose files -----------------------------------------------------------------------------------------------

/** A loose file with text typed into it and not saved: the window opens it in the editor over the file as it is. */
function typedInto(
  path: string,
  text: string,
  over: Partial<NoteEntry> = {},
  baseHash?: string,
): void {
  addFile(path, "# Proposal\n", { unsaved: true, ...over });
  fake.loadPending.mockResolvedValue({
    path: "C:/Users/ana/Downloads/proposal.md",
    baseHash: baseHash ?? `h-${path}`,
    text,
    savedMs: 5,
  });
}

const IN_SOURCE = { view: { mode: "source", caret: null, scroll: null } } as const;

describe("loose files", () => {
  it("opens a file in Read, listed under Files, and not in the editor on its own", async () => {
    addNote("a.md", "# A\n");
    addFile(FILE, "# Proposal\n");
    openName = FILE;
    await start();
    expect(notes.current).toBe(FILE);
    expect(notes.isFile).toBe(true);
    expect(edit.active).toBe(false);
    expect(docs.path).toBe(FILE);
    expect(fake.noteOpened).toHaveBeenCalledWith(FILE);
    expect(notes.arrangement(NOW).files.map((n) => n.path)).toEqual([FILE]);
    expect(notes.arrangement(NOW).groups.flatMap((g) => g.notes.map((n) => n.path))).toEqual([
      "a.md",
    ]);
    expect(fake.saveNote).not.toHaveBeenCalled();
    expect(fake.saveDocument).not.toHaveBeenCalled();
  });

  it("opens a file that has text typed into it in the editor, with that text and the unsaved mark", async () => {
    typedInto(FILE, "# Proposal\nhalf a sentence", IN_SOURCE);
    openName = FILE;
    await start();
    expect(edit.active).toBe(true);
    expect(edit.dirty).toBe(true);
    expect(edit.initial?.buffer).toBe("# Proposal\nhalf a sentence");
    expect(edit.diskHash).toBe(`h-${FILE}`);
    expect(notes.looseNotice).toBeNull();
  });

  it("tells the person when the file is not the version the text was typed over", async () => {
    typedInto(FILE, "# Proposal\nhalf a sentence", IN_SOURCE, "an-older-hash");
    openName = FILE;
    await start();
    expect(edit.dirty).toBe(true);
    expect(notes.looseNotice?.kind).toBe("changed");
  });

  it("keeps the version the text was typed over, so a save cannot replace a change nobody saw, with a keystroke or a restart between", async () => {
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE, "an-older-hash");
    openName = FILE;
    await start();
    expect(notes.looseNotice?.kind).toBe("changed");
    // Ctrl+S with the bar up: the save is guarded with the version the text was typed over, which is not the file's any more,
    // so the backend writes nothing and the bar stays
    fake.saveDocument.mockResolvedValue({ result: "conflict", diskHash: `h-${FILE}` });
    expect(await edit.save()).toBe(false);
    expect(fake.saveDocument).toHaveBeenCalledWith(FILE, "# Proposal\nhalf", "an-older-hash");
    expect(notes.looseNotice?.kind).toBe("changed");
    // one keystroke: the text is kept over the same version, so after a restart the bar is there again
    type("# Proposal\nhalf more");
    await vi.advanceTimersByTimeAsync(800);
    expect(fake.savePending).toHaveBeenLastCalledWith(
      FILE,
      "# Proposal\nhalf more",
      "an-older-hash",
    );
    expect(edit.diskHash).toBe("an-older-hash");
    // "Keep mine" is what makes the file's version the one that is replaced, and it asks once more
    await notes.keepMine();
    platform.askYesNo.mockResolvedValue(true);
    fake.saveDocument.mockImplementation(async () => ({
      result: "saved",
      hash: "h-mine",
      modifiedMs: NOW,
    }));
    expect(await edit.save()).toBe(true);
    expect(platform.askYesNo).toHaveBeenCalledTimes(1);
    expect(fake.saveDocument).toHaveBeenLastCalledWith(FILE, "# Proposal\nhalf more", `h-${FILE}`);
  });

  it("keeps what is typed apart from the file and writes the file only when asked", async () => {
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE);
    openName = FILE;
    await start();
    type("# Proposal\nhalf a sentence");
    await vi.advanceTimersByTimeAsync(800);
    expect(fake.savePending).toHaveBeenCalledWith(FILE, "# Proposal\nhalf a sentence", `h-${FILE}`);
    expect(fake.saveDocument).not.toHaveBeenCalled();
    expect(fake.saveNote).not.toHaveBeenCalled();
    expect(notes.entries.find((n) => n.path === FILE)?.unsaved).toBe(true);

    // Ctrl+S is what writes it
    expect(await edit.save()).toBe(true);
    expect(fake.saveDocument).toHaveBeenCalledWith(
      FILE,
      "# Proposal\nhalf a sentence",
      `h-${FILE}`,
    );
    expect(edit.dirty).toBe(false);
    expect(notes.entries.find((n) => n.path === FILE)?.unsaved).toBe(false);
  });

  it("forgets the kept text when it becomes the file's own text again", async () => {
    typedInto(FILE, "# Proposal\nmore", IN_SOURCE);
    openName = FILE;
    await start();
    type("# Proposal\n");
    await vi.advanceTimersByTimeAsync(800);
    expect(edit.dirty).toBe(false);
    expect(fake.clearPending).toHaveBeenCalledWith(FILE);
    expect(notes.entries.find((n) => n.path === FILE)?.unsaved).toBe(false);
  });

  it("keeps the text apart when another note is opened, and never asks", async () => {
    addNote("a.md", "# A\n");
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE);
    openName = FILE;
    await start();
    type("# Proposal\nhalf a sentence");
    expect(await notes.openNote("a.md")).toBe(true);
    expect(notes.current).toBe("a.md");
    expect(fake.savePending).toHaveBeenCalledWith(FILE, "# Proposal\nhalf a sentence", `h-${FILE}`);
    expect(platform.askUnsaved).not.toHaveBeenCalled();
    expect(fake.saveDocument).not.toHaveBeenCalled();
  });

  it("keeps the text apart when the window closes, and never asks", async () => {
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE);
    openName = FILE;
    await start();
    type("# Proposal\nhalf a sentence");
    expect(await edit.confirmLeave("close")).toBe(true);
    expect(fake.savePending).toHaveBeenCalledTimes(1);
    expect(platform.askUnsaved).not.toHaveBeenCalled();
  });

  it("asks, as a document does, before leaving the editor for Read with text that was not saved", async () => {
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE);
    openName = FILE;
    await start();
    edit.requestExit();
    expect(edit.prompt).not.toBeNull();
    // discarding it means it is not kept either
    edit.promptDiscard();
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.clearPending).toHaveBeenCalledWith(FILE);
  });

  it("shows the bar when the file changed and keeps what is shown; Reload keeps the typed text as a note first", async () => {
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE);
    openName = FILE;
    await start();
    type("# Proposal\nhalf a sentence");
    looseState({ state: "changed", stamp: "10-2" });
    await notes.checkLoose();
    expect(notes.looseNotice).toEqual({ path: FILE, kind: "changed", stamp: "10-2" });
    expect(edit.exactText()).toBe("# Proposal\nhalf a sentence");

    disk.set(FILE, { text: "# Theirs\n", hash: "h-theirs" });
    fake.keepEditsAsNote.mockResolvedValue("proposal (my edits).md");
    await notes.reloadFile();
    expect(fake.keepEditsAsNote).toHaveBeenCalledWith(FILE, "# Proposal\nhalf a sentence");
    expect(edit.exactText()).toBe("# Theirs\n");
    expect(edit.dirty).toBe(false);
    expect(notes.looseNotice).toBeNull();
  });

  it("keeps what is typed over the version on disk, and asks once more before it replaces that version", async () => {
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE);
    openName = FILE;
    await start();
    type("# Proposal\nmine");
    looseState({ state: "changed", stamp: "10-2" });
    await notes.checkLoose();
    disk.set(FILE, { text: "# Theirs\n", hash: "h-theirs" });
    await notes.keepMine();
    expect(notes.looseNotice).toBeNull();
    expect(edit.exactText()).toBe("# Proposal\nmine");
    expect(fake.saveDocument).not.toHaveBeenCalled();

    platform.askYesNo.mockResolvedValue(false);
    expect(await edit.save()).toBe(false);
    expect(fake.saveDocument).not.toHaveBeenCalled();

    platform.askYesNo.mockResolvedValue(true);
    expect(await edit.save()).toBe(true);
    expect(fake.saveDocument).toHaveBeenCalledWith(FILE, "# Proposal\nmine", "h-theirs");
  });

  it("does not write over a version that changed meanwhile: the bar asks", async () => {
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE);
    openName = FILE;
    await start();
    type("# Proposal\nmine");
    fake.saveDocument.mockResolvedValue({ result: "conflict", diskHash: "h-theirs" });
    expect(await edit.save()).toBe(false);
    expect(notes.looseNotice?.kind).toBe("changed");
    expect(edit.dirty).toBe(true);
  });

  it("while only reading, 'Keep mine' leaves that version of the change alone until the file changes again", async () => {
    addFile(FILE, "# Proposal\n");
    openName = FILE;
    await start();
    looseState({ state: "changed", stamp: "10-2" });
    await notes.checkLoose();
    expect(notes.looseNotice?.kind).toBe("changed");
    await notes.keepMine();
    expect(notes.looseNotice).toBeNull();
    await notes.checkLoose();
    expect(notes.looseNotice).toBeNull();
    looseState({ state: "changed", stamp: "10-3" });
    await notes.checkLoose();
    expect(notes.looseNotice).toEqual({ path: FILE, kind: "changed", stamp: "10-3" });
  });

  it("reloads a changed file by itself only when the setting is on and nothing was typed into it", async () => {
    addFile(FILE, "# Proposal\n");
    openName = FILE;
    await start();
    // off unless the person turns it on
    expect(notes.autoReload).toBe(false);
    looseState({ state: "changed", stamp: "10-2" });
    await notes.checkLoose();
    expect(notes.looseNotice?.kind).toBe("changed");
    await notes.keepMine();

    notes.setAutoReload(true);
    fake.readDocument.mockClear();
    looseState({ state: "changed", stamp: "10-3" });
    await notes.checkLoose();
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.readDocument).toHaveBeenCalled();
    expect(notes.looseNotice).toBeNull();
    notes.setAutoReload(false);
  });

  it("asks even with the setting on when text was typed into the file", async () => {
    typedInto(FILE, "# Proposal\nhalf", IN_SOURCE);
    openName = FILE;
    await start();
    notes.setAutoReload(true);
    looseState({ state: "changed", stamp: "10-2" });
    await notes.checkLoose();
    await vi.advanceTimersByTimeAsync(0);
    expect(notes.looseNotice?.kind).toBe("changed");
    expect(edit.exactText()).toBe("# Proposal\nhalf");
    expect(fake.keepEditsAsNote).not.toHaveBeenCalled();
    notes.setAutoReload(false);
  });

  it("reloads a file that is only being read by reading it again", async () => {
    addFile(FILE, "# Proposal\n");
    openName = FILE;
    await start();
    looseState({ state: "changed", stamp: "10-2" });
    await notes.checkLoose();
    fake.readDocument.mockClear();
    await notes.reloadFile();
    expect(fake.readDocument).toHaveBeenCalled();
    expect(fake.keepEditsAsNote).not.toHaveBeenCalled();
    expect(notes.looseNotice).toBeNull();
  });

  it("dims a file that is gone, and says what can be done when it is the one on screen", async () => {
    addFile(FILE, "# Proposal\n");
    addFile(OTHER, "buy milk\n");
    openName = FILE;
    await start();
    looseState({ state: "missing", stamp: "" }, { path: OTHER, state: "same" });
    await notes.checkLoose();
    expect(notes.entries.find((n) => n.path === FILE)?.missing).toBe(true);
    expect(notes.entries.find((n) => n.path === OTHER)?.missing).toBe(false);
    expect(notes.looseNotice?.kind).toBe("missing");
    // it came back
    looseState({ state: "same" }, { path: OTHER, state: "same" });
    await notes.checkLoose();
    expect(notes.entries.find((n) => n.path === FILE)?.missing).toBe(false);
    expect(notes.looseNotice).toBeNull();
  });

  it("follows a file that was renamed in its folder", async () => {
    addFile(FILE, "# Proposal\n");
    openName = FILE;
    await start();
    const renamed = "@file/0a1b2c3d/proposal v2.md";
    disk.set(renamed, disk.get(FILE) ?? { text: "", hash: "" });
    looseState({ path: renamed, was: FILE });
    fake.listNotes.mockClear();
    await notes.checkLoose();
    expect(notes.current).toBe(renamed);
    expect(fake.listNotes).toHaveBeenCalled();
  });

  it("looks at nothing while no file is listed", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    await notes.checkLoose();
    expect(fake.looseStatus).not.toHaveBeenCalled();
  });

  it("takes a file out of the list without deleting it, and asks first when text of it was not saved", async () => {
    addNote("a.md", "# A\n");
    addFile(FILE, "# Proposal\n", { unsaved: true });
    openName = "a.md";
    await start();
    platform.askYesNo.mockResolvedValue(false);
    await notes.removeFile(FILE);
    expect(fake.removeLooseFile).not.toHaveBeenCalled();
    platform.askYesNo.mockResolvedValue(true);
    await notes.removeFile(FILE);
    expect(fake.removeLooseFile).toHaveBeenCalledWith(FILE);
    expect(fake.deleteNotes).not.toHaveBeenCalled();
  });

  it("takes a file out of the list, and deletes nothing, when Delete is pressed on it", async () => {
    addNote("a.md", "# A\n");
    addFile(FILE, "# Proposal\n");
    openName = "a.md";
    await start();
    await notes.deleteNotes([FILE]);
    expect(fake.removeLooseFile).toHaveBeenCalledWith(FILE);
    expect(fake.deleteNotes).not.toHaveBeenCalled();
  });

  it("opens the file that a launch or another window carries", async () => {
    addNote("a.md", "# A\n");
    addFile(FILE, "# Proposal\n");
    fake.openLooseFile.mockResolvedValue({ kind: "file", path: FILE });
    request = { newNote: false, open: null, openFile: "C:/Users/ana/Downloads/proposal.md" };
    await start();
    expect(fake.openLooseFile).toHaveBeenCalledWith("C:/Users/ana/Downloads/proposal.md");
    expect(notes.current).toBe(FILE);
    // and while it runs
    await notes.handleRequest({ newNote: false, open: null, openFile: "D:/more.md" });
    expect(fake.openLooseFile).toHaveBeenCalledWith("D:/more.md");
  });

  it("says why a file could not be opened and goes on to what it would have shown", async () => {
    addNote("a.md", "# A\n");
    openName = "a.md";
    fake.openLooseFile.mockRejectedValue({ kind: "binaryFile", message: "x", subject: "x.png" });
    request = { newNote: false, open: null, openFile: "C:/x.png" };
    await start();
    expect(notes.current).toBe("a.md");
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
  });

  it("shows a file that is not there as such, and its kept text can be saved as a note", async () => {
    addFile(FILE, "", { missing: true, unsaved: true });
    fake.loadPending.mockResolvedValue({
      path: "C:/Users/ana/Downloads/proposal.md",
      baseHash: "h",
      text: "what I typed",
      savedMs: 5,
    });
    openName = FILE;
    await start();
    expect(notes.currentMissing).toBe(true);
    expect(edit.active).toBe(false);
    expect(notes.looseNotice?.kind).toBe("missing");
    expect(notes.missingKept?.text).toBe("what I typed");

    addNote("proposal (my edits).md", "what I typed");
    fake.keepEditsAsNote.mockResolvedValue("proposal (my edits).md");
    await notes.saveMissingAsNote();
    expect(fake.keepEditsAsNote).toHaveBeenCalledWith(FILE, "what I typed");
    expect(notes.current).toBe("proposal (my edits).md");
  });

  it("writes a file that is not there again, with the text kept for it", async () => {
    addFile(FILE, "", { missing: true, unsaved: true });
    fake.loadPending.mockResolvedValue({
      path: "C:/Users/ana/Downloads/proposal.md",
      baseHash: "h",
      text: "what I typed",
      savedMs: 5,
    });
    // the file is written again: it is there, and nothing is kept for it any more
    fake.createDocument.mockImplementation(async () => {
      list = list.map((n) => ({ ...n, missing: false, unsaved: false }));
      disk.set(FILE, { text: "what I typed", hash: "h-new" });
      fake.loadPending.mockResolvedValue(null);
      return { path: FILE, hash: "h-new", modifiedMs: NOW };
    });
    openName = FILE;
    await start();
    await notes.saveMissingAgain();
    expect(fake.createDocument).toHaveBeenCalledWith(FILE, "what I typed");
    expect(notes.missingKept).toBeNull();
    expect(notes.looseNotice).toBeNull();
  });

  it("edits a plain text file in Source only", async () => {
    typedInto(OTHER, "buy milk\nand eggs", { view: { mode: "visual", caret: null, scroll: null } });
    openName = OTHER;
    await start();
    expect(edit.active).toBe(true);
    expect(edit.mode).toBe("source");
    expect(edit.sourceOnly).toBe(true);
    expect(edit.blocked).toBeNull();
    expect(await edit.switchTo("visual")).toBe(false);
  });

  it("keeps a pin on a file like a note's", async () => {
    addFile(FILE, "# Proposal\n", { pinned: true });
    addNote("a.md", "# A\n");
    openName = "a.md";
    await start();
    expect(notes.arrangement(NOW).pinned.map((n) => n.path)).toEqual([FILE]);
    expect(notes.order(NOW)).toEqual([FILE, "a.md"]);
    await notes.togglePin(FILE);
    expect(fake.setNotePinned).toHaveBeenCalledWith(FILE, false);
  });

  it("copies the text of a file, and offers nothing to delete: the file is not ours", async () => {
    addFile(FILE, "# Proposal\ntext\n");
    openName = FILE;
    await start();
    await notes.copyNote();
    expect(platform.copyText).toHaveBeenCalledWith("# Proposal\ntext\n");
    expect(fake.markNoteCopied).not.toHaveBeenCalled();
    expect(toast.current).toMatchObject({ type: "message" });
    expect(
      toast.current && "action" in toast.current ? toast.current.action : undefined,
    ).toBeUndefined();
  });
});
