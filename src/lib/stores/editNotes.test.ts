// The edit store in the Notes window's mode (docs/design/v0.5.md section 4): the text is saved as it is typed, nothing ever
// asks "Save changes?", and a note that changed elsewhere is never overwritten. No test waits for time: the fake clock is moved.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../bridge";
import type { DocumentView, NoteSaved, ParsedBlocks, SourceDoc } from "../types";

const fake = vi.hoisted(() => ({
  documentSource: vi.fn(),
  parseBlocks: vi.fn(),
  saveNote: vi.fn(),
  newNote: vi.fn(),
  saveDocument: vi.fn(),
  readDocument: vi.fn(),
  fileMeta: vi.fn(),
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

import { QUIET_MS } from "../autosave";
import { bridge } from "../bridge";
import { noOutline } from "../editor/outlineStub";
import { mockParseBlocks } from "../mock/mockBlocks";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { session } from "./session.svelte";

const PATH = "2026-10-15 1432.md";

const view = (path: string): DocumentView => ({
  path,
  kind: "markdown",
  html: "",
  blocks: [],
  title: null,
  frontmatter: null,
  modifiedMs: 1,
  sizeBytes: 1,
  hash: null,
  encodingNote: null,
});

const source = (path: string, text: string, hash: string): SourceDoc => ({
  path,
  text,
  hash,
  modifiedMs: 100,
  ...mockParseBlocks(text),
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
    caretLine: () => 0,
    topLine: () => 0,
    focus: vi.fn(),
    insertImage: () => {},
    applyChanges: () => {},
  };
  return editor;
}

/** Types into the fake editor the way CodeMirror reports it. */
function type(editor: ReturnType<typeof fakeEditor>, text: string): void {
  editor.text = text;
  edit.noteChange(text.length);
}

const saved = (hash: string, path = PATH): NoteSaved => ({
  result: "saved",
  path,
  hash,
  modifiedMs: 200,
  created: false,
});

async function openNote(text = "# Plan\n", hash = "h1", path = PATH) {
  docs.current = view(path);
  fake.documentSource.mockResolvedValue(source(path, text, hash));
  expect(await edit.enter(false, "source")).toBe(true);
  const editor = fakeEditor(edit.initial?.buffer ?? "");
  bridge.editor = editor;
  return editor;
}

beforeEach(() => {
  vi.useFakeTimers();
  session.editMode = "source";
  edit.reset();
  edit.noteHooks = {};
  edit.autosave = true;
  edit.readOnlyReason = null;
  docs.reset();
  bridge.editor = undefined;
  for (const mock of Object.values(fake)) mock.mockReset();
  for (const mock of Object.values(platform)) {
    if (typeof mock === "function" && "mockClear" in mock)
      (mock as { mockClear(): void }).mockClear();
  }
  fake.parseBlocks.mockImplementation(async (text: string): Promise<ParsedBlocks> =>
    mockParseBlocks(text),
  );
  docs.guard = (resume) => edit.holdNavigation(resume);
});
afterEach(() => {
  edit.autosave = false;
  edit.reset();
  vi.useRealTimers();
});

describe("autosave", () => {
  it("saves 750 ms after typing stops, with the hash the note was opened with", async () => {
    const editor = await openNote("# Plan\n", "h1");
    fake.saveNote.mockResolvedValue(saved("h2"));
    type(editor, "# Plan\n\nmore");
    await vi.advanceTimersByTimeAsync(QUIET_MS - 1);
    expect(fake.saveNote).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(fake.saveNote).toHaveBeenCalledTimes(1);
    expect(fake.saveNote).toHaveBeenCalledWith(PATH, "# Plan\n\nmore", "h1");
    expect(edit.dirty).toBe(false);
    expect(fake.saveDocument).not.toHaveBeenCalled();
    // the next save goes on from the version just written
    type(editor, "# Plan\n\nmore, more");
    await vi.advanceTimersByTimeAsync(QUIET_MS);
    expect(fake.saveNote).toHaveBeenLastCalledWith(PATH, "# Plan\n\nmore, more", "h2");
  });

  it("saves at least every 3 s while typing goes on", async () => {
    const editor = await openNote();
    fake.saveNote.mockResolvedValue(saved("h2"));
    for (let i = 1; i <= 20; i++) {
      type(editor, `# Plan\n${"x".repeat(i)}`);
      await vi.advanceTimersByTimeAsync(200);
    }
    expect(fake.saveNote).toHaveBeenCalledTimes(1);
  });

  it("saves nothing when nothing changed, and has no 'saved' note in the header", async () => {
    const editor = await openNote();
    fake.saveNote.mockResolvedValue(saved("h2"));
    type(editor, "# Plan\nx");
    type(editor, "# Plan\n");
    await vi.advanceTimersByTimeAsync(10_000);
    expect(fake.saveNote).not.toHaveBeenCalled();
    type(editor, "# Plan\ny");
    await edit.flush();
    expect(edit.note).toBeNull();
    expect(edit.hint).toBe(false);
  });

  it("flush saves at once and says whether the note is saved", async () => {
    const editor = await openNote();
    fake.saveNote.mockResolvedValue(saved("h2"));
    type(editor, "# Plan\nnew");
    expect(await edit.flush()).toBe(true);
    expect(fake.saveNote).toHaveBeenCalledTimes(1);
    // nothing to save: nothing is asked
    expect(await edit.flush()).toBe(true);
    expect(fake.saveNote).toHaveBeenCalledTimes(1);
    // a save that fails leaves the note unsaved, with the reason on the bar
    type(editor, "# Plan\nnewer");
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: PATH });
    expect(await edit.flush()).toBe(false);
    expect(edit.saveError).not.toBeNull();
    expect(edit.dirty).toBe(true);
  });

  it("flush tries again after a save that failed, so a note whose problem is gone is saved", async () => {
    const editor = await openNote();
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: PATH });
    type(editor, "# Plan\nnewer");
    expect(await edit.flush()).toBe(false);
    expect(edit.saveError).not.toBeNull();
    fake.saveNote.mockResolvedValue(saved("h2"));
    expect(await edit.flush()).toBe(true);
    expect(fake.saveNote).toHaveBeenLastCalledWith(PATH, "# Plan\nnewer", "h1");
    expect(edit.saveError).toBeNull();
    expect(edit.dirty).toBe(false);
  });

  it("flushes nothing while no note is open", async () => {
    expect(await edit.flush()).toBe(true);
    expect(fake.saveNote).not.toHaveBeenCalled();
  });
});

describe("never asking", () => {
  it("moving to another note saves first, then goes on, without a question", async () => {
    const editor = await openNote();
    fake.saveNote.mockResolvedValue(saved("h2"));
    type(editor, "# Plan\nunsaved");
    const resume = vi.fn();
    expect(edit.holdNavigation(resume)).toBe(true);
    expect(edit.prompt).toBeNull();
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.saveNote).toHaveBeenCalledWith(PATH, "# Plan\nunsaved", "h1");
    expect(resume).toHaveBeenCalledTimes(1);
    expect(edit.active).toBe(false);
    expect(platform.askUnsaved).not.toHaveBeenCalled();
  });

  it("a clean note is left at once", async () => {
    await openNote();
    const resume = vi.fn();
    expect(edit.holdNavigation(resume)).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(edit.active).toBe(false);
  });

  it("a note that cannot be saved is not left", async () => {
    const editor = await openNote();
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: PATH });
    type(editor, "# Plan\nunsaved");
    const resume = vi.fn();
    expect(edit.holdNavigation(resume)).toBe(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(resume).not.toHaveBeenCalled();
    expect(edit.active).toBe(true);
    expect(edit.dirty).toBe(true);
  });

  it("Read, Done and Esc save and leave editing without a bar", async () => {
    const editor = await openNote();
    fake.saveNote.mockResolvedValue(saved("h2"));
    fake.readDocument.mockResolvedValue(view(PATH));
    type(editor, "# Plan\nunsaved");
    edit.requestExit();
    await vi.advanceTimersByTimeAsync(0);
    expect(edit.prompt).toBeNull();
    expect(fake.saveNote).toHaveBeenCalledTimes(1);
    expect(edit.active).toBe(false);
  });

  it("closing the window saves first and lets it close", async () => {
    const editor = await openNote();
    fake.saveNote.mockResolvedValue(saved("h2"));
    type(editor, "# Plan\nunsaved");
    expect(await edit.confirmLeave("close")).toBe(true);
    expect(fake.saveNote).toHaveBeenCalledTimes(1);
    expect(platform.askUnsaved).not.toHaveBeenCalled();
  });

  it("an update restart saves first too, and waits when the note cannot be saved", async () => {
    const editor = await openNote();
    fake.saveNote.mockResolvedValue(saved("h2"));
    type(editor, "# Plan\nunsaved");
    expect(await edit.confirmLeave("restart")).toBe(true);
    type(editor, "# Plan\nmore");
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: PATH });
    expect(await edit.confirmLeave("restart")).toBe(false);
  });

  it("a note that cannot be saved keeps the window open so the bar is seen, and the next try saves it once the problem is gone", async () => {
    const editor = await openNote();
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: PATH });
    type(editor, "# Plan\nunsaved");
    expect(await edit.confirmLeave("close")).toBe(false);
    expect(edit.saveError).not.toBeNull();
    // the file was made writable again: closing writes the text (it is not forgotten with the save that failed)
    fake.saveNote.mockResolvedValue(saved("h2"));
    expect(await edit.confirmLeave("close")).toBe(true);
    expect(fake.saveNote).toHaveBeenLastCalledWith(PATH, "# Plan\nunsaved", "h1");
    expect(edit.dirty).toBe(false);
  });

  it("when the note still cannot be written the second close keeps the text as a note of its own", async () => {
    const editor = await openNote();
    const denied = { kind: "denied", message: "no", subject: PATH };
    fake.saveNote.mockImplementation(async (path: string) => {
      if (path === PATH) throw denied;
      return saved("n1", path);
    });
    fake.newNote.mockResolvedValue({ path: "2026-10-15 1500.md", hash: "e0", modifiedMs: 100 });
    type(editor, "# Plan\nunsaved");
    expect(await edit.confirmLeave("close")).toBe(false);
    expect(fake.newNote).not.toHaveBeenCalled();
    expect(await edit.confirmLeave("close")).toBe(true);
    // a new note is an empty file: the text is saved over it, guarded by its hash
    expect(fake.saveNote).toHaveBeenLastCalledWith("2026-10-15 1500.md", "# Plan\nunsaved", "e0");
  });

  it("closing is refused once more when not even a copy could be written, and the third try closes", async () => {
    const editor = await openNote();
    fake.saveNote.mockRejectedValue({ kind: "denied", message: "no", subject: PATH });
    fake.newNote.mockResolvedValue({ path: "2026-10-15 1500.md", hash: "e0", modifiedMs: 100 });
    type(editor, "# Plan\nunsaved");
    expect(await edit.confirmLeave("close")).toBe(false);
    expect(await edit.confirmLeave("close")).toBe(false);
    expect(await edit.confirmLeave("close")).toBe(true);
  });
});

describe("a note that changed somewhere else", () => {
  it("is not overwritten: the text is kept as a copy and the editor shows what is on disk", async () => {
    const editor = await openNote("# Plan\n", "h1");
    const copies: string[] = [];
    edit.noteHooks.conflict = (copy) => copies.push(copy);
    fake.saveNote.mockResolvedValue({
      result: "conflict",
      copy: "Plan (conflict 2026-10-15 1432).md",
    });
    fake.documentSource.mockResolvedValue(source(PATH, "# Plan\n\ntheirs\n", "h9"));
    type(editor, "# Plan\n\nmine");
    expect(await edit.flush()).toBe(true);
    expect(copies).toEqual(["Plan (conflict 2026-10-15 1432).md"]);
    expect(editor.text).toBe("# Plan\n\ntheirs\n");
    expect(edit.dirty).toBe(false);
    expect(edit.conflict).toBeNull();
    // and it goes on from their version
    fake.saveNote.mockResolvedValue(saved("h10"));
    type(editor, "# Plan\n\ntheirs\nand mine");
    await edit.flush();
    expect(fake.saveNote).toHaveBeenLastCalledWith(PATH, "# Plan\n\ntheirs\nand mine", "h9");
  });

  it("a change on disk while the note has unsaved text keeps that text as a copy, with no bar", async () => {
    const editor = await openNote("# Plan\n", "h1");
    type(editor, "# Plan\n\nmine");
    fake.documentSource.mockResolvedValue(source(PATH, "# Plan\n\ntheirs\n", "h9"));
    fake.saveNote.mockResolvedValue({ result: "conflict", copy: "copy.md" });
    edit.onFsChange({
      changes: [{ path: PATH, kind: "modified", atMs: 1 }],
      bulk: null,
      rescan: false,
    });
    await vi.advanceTimersByTimeAsync(1000);
    expect(fake.saveNote).toHaveBeenCalledWith(PATH, "# Plan\n\nmine", "h1");
    expect(edit.conflict).toBeNull();
    expect(editor.text).toBe("# Plan\n\ntheirs\n");
  });

  it("a change on disk while the note is clean is simply loaded", async () => {
    const editor = await openNote("# Plan\n", "h1");
    fake.documentSource.mockResolvedValue(source(PATH, "# Plan\n\nfrom another computer\n", "h9"));
    edit.onFsChange({
      changes: [{ path: PATH, kind: "modified", atMs: 1 }],
      bulk: null,
      rescan: false,
    });
    await vi.advanceTimersByTimeAsync(1000);
    expect(editor.text).toBe("# Plan\n\nfrom another computer\n");
    expect(fake.saveNote).not.toHaveBeenCalled();
    expect(edit.conflict).toBeNull();
    expect(edit.note).toBeNull();
  });

  it("a note deleted from the folder while it has unsaved text is written again", async () => {
    const editor = await openNote("# Plan\n", "h1");
    type(editor, "# Plan\n\nmine");
    fake.documentSource.mockRejectedValue({ kind: "notFound", message: "gone" });
    fake.saveNote.mockResolvedValue({ ...saved("h2"), created: true });
    edit.onFsChange({
      changes: [{ path: PATH, kind: "removed", atMs: 1 }],
      bulk: null,
      rescan: false,
    });
    await vi.advanceTimersByTimeAsync(1000);
    expect(fake.saveNote).toHaveBeenCalledWith(PATH, "# Plan\n\nmine", "h1");
    expect(edit.conflict).toBeNull();
  });

  it("a note deleted from the folder while it is clean is reported to the notes", async () => {
    await openNote("# Plan\n", "h1");
    const gone = vi.fn();
    edit.noteHooks.gone = gone;
    fake.documentSource.mockRejectedValue({ kind: "notFound", message: "gone" });
    edit.onFsChange({
      changes: [{ path: PATH, kind: "removed", atMs: 1 }],
      bulk: null,
      rescan: false,
    });
    await vi.advanceTimersByTimeAsync(1000);
    expect(gone).toHaveBeenCalledWith(PATH);
    expect(edit.conflict).toBeNull();
  });
});

describe("a new note", () => {
  it("is an empty file already: its first text is saved like any other, over the hash of the empty file", async () => {
    const editor = await openNote("", "e0", "2026-10-15 1432.md");
    const written = vi.fn();
    edit.noteHooks.written = written;
    fake.saveNote.mockResolvedValue(saved("n1", "2026-10-15 1432.md"));
    type(editor, "  hello");
    expect(await edit.flush()).toBe(true);
    expect(fake.saveNote).toHaveBeenCalledWith("2026-10-15 1432.md", "  hello", "e0");
    expect(written).toHaveBeenCalledWith({
      path: "2026-10-15 1432.md",
      created: false,
      text: "  hello",
    });
    // from then on it is guarded by the hash it was saved with
    fake.saveNote.mockResolvedValue(saved("n2", "2026-10-15 1432.md"));
    type(editor, "  hello world");
    await edit.flush();
    expect(fake.saveNote).toHaveBeenLastCalledWith("2026-10-15 1432.md", "  hello world", "n1");
  });

  it("with only blank text typed in is saved as that text: nothing is skipped and the note can be left", async () => {
    const editor = await openNote("", "e0", "2026-10-15 1432.md");
    fake.saveNote.mockResolvedValue(saved("n1", "2026-10-15 1432.md"));
    type(editor, "  \n");
    expect(await edit.flush()).toBe(true);
    expect(edit.dirty).toBe(false);
    expect(fake.saveNote).toHaveBeenCalledTimes(1);
    expect(fake.saveNote).toHaveBeenCalledWith("2026-10-15 1432.md", "  \n", "e0");
  });

  it("deleted by another program while it was open, comes back with its text, perhaps under another name", async () => {
    const editor = await openNote("# Plan\n", "h1", "2026-10-15 1432.md");
    const renamed = vi.fn();
    const written = vi.fn();
    edit.noteHooks.renamed = renamed;
    edit.noteHooks.written = written;
    fake.saveNote.mockResolvedValue({
      result: "saved",
      path: "2026-10-15 1432-2.md",
      hash: "n1",
      modifiedMs: 300,
      created: true,
    } satisfies NoteSaved);
    type(editor, "# Plan\nmore");
    await edit.flush();
    expect(renamed).toHaveBeenCalledWith("2026-10-15 1432.md", "2026-10-15 1432-2.md");
    expect(written).toHaveBeenCalledWith({
      path: "2026-10-15 1432-2.md",
      created: true,
      text: "# Plan\nmore",
    });
    expect(edit.path).toBe("2026-10-15 1432-2.md");
    fake.saveNote.mockResolvedValue(saved("n2", "2026-10-15 1432-2.md"));
    type(editor, "# Plan\nmore still");
    await edit.flush();
    expect(fake.saveNote).toHaveBeenLastCalledWith(
      "2026-10-15 1432-2.md",
      "# Plan\nmore still",
      "n1",
    );
  });
});

describe("reading only", () => {
  it("a note that can only be read says why and does not open an editor", async () => {
    edit.readOnlyReason = "These notes are open in another PerMark window.";
    docs.current = view(PATH);
    expect(await edit.enter()).toBe(false);
    expect(edit.active).toBe(false);
    expect(edit.blocked).toBe("These notes are open in another PerMark window.");
    expect(fake.documentSource).not.toHaveBeenCalled();
  });
});

describe("the Notes window's own mode is off in a project window", () => {
  it("saves with save_document and asks about unsaved work, as before", async () => {
    edit.autosave = false;
    const editor = await openNote("# Plan\n", "h1");
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    type(editor, "# Plan\nx");
    await vi.advanceTimersByTimeAsync(10_000);
    expect(fake.saveNote).not.toHaveBeenCalled();
    expect(fake.saveDocument).not.toHaveBeenCalled();
    expect(edit.holdNavigation(() => {})).toBe(true);
    expect(edit.prompt).not.toBeNull();
  });
});
