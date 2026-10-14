import { beforeEach, describe, expect, it, vi } from "vitest";
import type { VisualApi } from "../bridge";
import type { DocumentView, ParsedBlocks, SourceDoc } from "../types";

// Like edit.test.ts: the backend and the native dialogs are replaced; these tests are about what the store decides when the editor
// in use is the Visual one, and when it switches between the two.
const fake = vi.hoisted(() => ({
  documentSource: vi.fn(),
  parseBlocks: vi.fn(),
  saveDocument: vi.fn(),
  createDocument: vi.fn(),
  writeAsset: vi.fn(),
  readDocument: vi.fn(),
  fileMeta: vi.fn(),
}));
vi.mock("../api", () => ({
  api: {
    ...fake,
    metaIndex: async () => [],
    listTags: async () => ({ tags: [], untagged: 0, missing: 0, status: {} }),
    on: async () => () => {},
  },
}));
vi.mock("../platform", () => ({
  setWindowTitle: vi.fn(async () => {}),
  askUnsaved: vi.fn(async () => "keep"),
  askYesNo: vi.fn(async () => true),
  copyText: vi.fn(async () => true),
  guardWindowClose: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));
// node has no FileReader, which is how the browser turns a picture into base64
vi.mock("../editor/imageName", async (original) => ({
  ...(await original<typeof import("../editor/imageName")>()),
  toBase64: async (blob: Blob) => Buffer.from(await blob.arrayBuffer()).toString("base64"),
}));

import { bridge } from "../bridge";
import { noOutline } from "../editor/outlineStub";
import { mockParseBlocks } from "../mock/mockBlocks";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";

const PATH = "docs/a.md";
const ORIGINAL = "# A\n\nfirst\n\nsecond\n";
const EDITED_IN_SOURCE = "# A\n\nfirst, changed in Source\n\nsecond\n";

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
const source = (text: string, hash: string): SourceDoc => ({
  path: PATH,
  text,
  hash,
  modifiedMs: 100,
  ...mockParseBlocks(text),
});

/** A Visual editor that is a text, a flag and a record of what the store asked of it. */
type FakeVisual = VisualApi & {
  current: string;
  dirty: boolean;
  block: number | null;
  line: number;
  loads: unknown[];
  rebased: unknown[];
};

function fakeVisual(initial: string): FakeVisual {
  const editor: FakeVisual = {
    ...noOutline,
    mode: "visual",
    current: initial,
    dirty: false,
    block: 0,
    line: 0,
    loads: [],
    rebased: [],
    text: () => editor.current,
    status: () => ({ dirty: editor.dirty, changed: editor.dirty ? 1 : 0 }),
    caretLine: () => editor.line,
    caretBlock: () => editor.block,
    lineOfBlock: (block: number | null) => (block === null ? editor.line : block * 2),
    load: (input: unknown) => void editor.loads.push(input),
    rebase: (map: unknown) => void editor.rebased.push(map),
    focus: vi.fn(),
    insertImage: vi.fn(),
  };
  return editor;
}

/** A Source editor that is only a string. */
function fakeSource(initial: string) {
  const editor = {
    ...noOutline,
    mode: "source" as const,
    text: initial,
    line: 0,
    getText: () => editor.text,
    setText: (text: string, line: number) => {
      editor.text = text;
      editor.line = line;
    },
    caretLine: () => editor.line,
    focus: vi.fn(),
    insertImage: vi.fn(),
    applyChanges: vi.fn(),
  };
  return editor;
}

async function startVisual(text = ORIGINAL, hash = "h1") {
  docs.current = view(PATH);
  fake.documentSource.mockResolvedValue(source(text, hash));
  expect(await edit.enter()).toBe(true);
  const editor = fakeVisual(text);
  bridge.editor = editor;
  return editor;
}

/** Starts in Source mode with a fake Source editor holding the file's text. */
async function startSource() {
  session.editMode = "source";
  docs.current = view(PATH);
  fake.documentSource.mockResolvedValue(source(ORIGINAL, "h1"));
  await edit.enter();
  const editor = fakeSource(edit.initial?.buffer ?? "");
  bridge.editor = editor;
  return editor;
}

/** A `parse_blocks` answer that is given when the test says so. */
function deferredBlocks(): { release: (parsed: ParsedBlocks) => void } {
  const handle = { release: (_parsed: ParsedBlocks): void => {} };
  fake.parseBlocks.mockImplementationOnce(
    () =>
      new Promise<ParsedBlocks>((resolve) => {
        handle.release = resolve;
      }),
  );
  return handle;
}

beforeEach(() => {
  session.editMode = "visual";
  edit.reset();
  docs.reset();
  bridge.editor = undefined;
  bridge.topLine = undefined;
  for (const mock of Object.values(fake)) mock.mockReset();
  fake.parseBlocks.mockImplementation(async (text: string): Promise<ParsedBlocks> =>
    mockParseBlocks(text),
  );
  docs.guard = (resume) => edit.holdNavigation(resume);
});

describe("entering in Visual mode", () => {
  it("opens the visual editor from the blocks of the file, with the caret on the block at the top of the viewport", async () => {
    bridge.topLine = () => 3;
    await startVisual();
    expect(edit.mode).toBe("visual");
    expect(edit.initial).toBeNull();
    expect(edit.visualStart?.map.blocks.map((b) => b.text)).toEqual(["# A", "first", "second"]);
    expect(edit.visualStart?.block).toBe(1);
    expect(edit.visualStart?.dirty).toBe(false);
    expect(edit.dirty).toBe(false);
  });

  it("opens in Source when that is the setting", async () => {
    session.editMode = "source";
    await startVisual();
    expect(edit.mode).toBe("source");
    expect(edit.visualStart).toBeNull();
    expect(edit.initial?.buffer).toBe(ORIGINAL);
  });

  it("follows what the editor says about unsaved work", async () => {
    const editor = await startVisual();
    editor.dirty = true;
    edit.noteVisual();
    expect(edit.dirty).toBe(true);
    expect(edit.unsavedBlocks).toBe(1);
    editor.dirty = false;
    edit.noteVisual();
    expect(edit.dirty).toBe(false);
    expect(edit.unsavedBlocks).toBe(0);
  });
});

describe("saving from Visual mode", () => {
  it("writes the text the editor composes, with the hash it started from", async () => {
    const editor = await startVisual();
    editor.current = "# A\n\nfirst, edited\n\nsecond\n";
    editor.dirty = true;
    edit.noteVisual();
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    expect(await edit.save()).toBe(true);
    expect(fake.saveDocument).toHaveBeenCalledWith(PATH, "# A\n\nfirst, edited\n\nsecond\n", "h1");
  });

  it("starts the editor over from the saved blocks, so the unsaved bars clear", async () => {
    const editor = await startVisual();
    editor.current = "# A\n\nfirst, edited\n\nsecond\n";
    editor.dirty = true;
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    await edit.save();
    expect(editor.rebased).toHaveLength(1);
    // the real editor reports clean once rebased
    editor.dirty = false;
    edit.noteVisual();
    expect(edit.dirty).toBe(false);
  });

  it("leaves the document alone when the person typed while the file was being written", async () => {
    const editor = await startVisual();
    editor.current = "# A\n\nfirst, edited\n\nsecond\n";
    editor.dirty = true;
    fake.saveDocument.mockImplementation(async () => {
      // typing during the write
      editor.current = "# A\n\nfirst, edited more\n\nsecond\n";
      return { result: "saved", hash: "h2", modifiedMs: 200 };
    });
    await edit.save();
    expect(editor.rebased).toHaveLength(0);
    expect(edit.dirty).toBe(true);
  });

  it("leaves the document alone, and says so, when the blocks of the saved text cannot be read", async () => {
    const editor = await startVisual();
    editor.current = "# A\n\nfirst, edited\n\nsecond\n";
    editor.dirty = true;
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    fake.parseBlocks.mockRejectedValue(new Error("backend down"));
    const warn = vi.spyOn(toast, "message");
    expect(await edit.save()).toBe(true);
    // a Visual document started over from no blocks would save with every blank line gone: it keeps the blocks it has
    expect(editor.rebased).toHaveLength(0);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("backend down"), "warn");
  });

  it("starts over from no blocks when the saved text has none to read", async () => {
    const editor = await startVisual();
    editor.current = "\n";
    editor.dirty = true;
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    fake.parseBlocks.mockRejectedValue(new Error("backend down"));
    expect(await edit.save()).toBe(true);
    expect(editor.rebased).toHaveLength(1);
  });

  it("a conflict writes nothing and keeps the document", async () => {
    const editor = await startVisual();
    editor.current = "# A\n\nmine\n\nsecond\n";
    editor.dirty = true;
    fake.saveDocument.mockResolvedValue({ result: "conflict", diskHash: "h9" });
    expect(await edit.save()).toBe(false);
    expect(edit.conflict?.diskHash).toBe("h9");
    expect(editor.rebased).toHaveLength(0);
  });
});

describe("the file changed on disk while editing in Visual mode", () => {
  const CHANGED = "# A\n\nnew first line\n\nfirst\n\nsecond\n";
  const change = {
    changes: [{ kind: "modified" as const, path: PATH, atMs: 1 }],
    bulk: null,
    rescan: false,
  };

  it("reloads a clean document into the editor, keeping the caret on the same block", async () => {
    const editor = await startVisual();
    editor.block = 2; // "second"
    fake.documentSource.mockResolvedValue(source(CHANGED, "h2"));
    edit.onFsChange(change);
    await vi.waitFor(() => expect(editor.loads).toHaveLength(1));
    const input = editor.loads[0] as {
      map: { blocks: { text: string }[] };
      block: number;
      dirty: boolean;
    };
    expect(input.map.blocks.map((b) => b.text)).toEqual([
      "# A",
      "new first line",
      "first",
      "second",
    ]);
    // "second" is block 2 before and block 3 after
    expect(input.block).toBe(3);
    expect(input.dirty).toBe(false);
    expect(edit.note).toMatch(/Reloaded/);
  });

  it("raises the conflict for unsaved work, and Reload theirs puts the file's blocks in the editor", async () => {
    const editor = await startVisual();
    editor.dirty = true;
    edit.noteVisual();
    fake.documentSource.mockResolvedValue(source(CHANGED, "h2"));
    edit.onFsChange(change);
    await vi.waitFor(() => expect(edit.conflict).not.toBeNull());
    expect(editor.loads).toHaveLength(0);
    await edit.reloadTheirs(true);
    expect(editor.loads).toHaveLength(1);
    expect(edit.conflict).toBeNull();
  });
});

describe("switching between Visual and Source", () => {
  it("Visual to Source opens the composed text in the buffer, on the line of the caret's block, with unsaved work kept", async () => {
    const editor = await startVisual();
    editor.current = "# A\n\nfirst, edited\n\nsecond\n";
    editor.dirty = true;
    editor.line = 2;
    expect(await edit.switchTo("source")).toBe(true);
    expect(edit.mode).toBe("source");
    expect(edit.initial).toEqual({
      buffer: "# A\n\nfirst, edited\n\nsecond\n",
      line: 2,
      atEnd: false,
    });
    expect(edit.dirty).toBe(true);
    expect(edit.visualStart).toBeNull();
  });

  it("Visual to Source for the button of a locked block puts the caret on that block", async () => {
    const editor = await startVisual();
    editor.line = 0;
    await edit.switchTo("source", { block: 2 });
    expect(edit.initial?.line).toBe(4);
  });

  it("Visual to Source with nothing edited leaves the buffer clean", async () => {
    await startVisual();
    await edit.switchTo("source");
    expect(edit.dirty).toBe(false);
  });

  it("Source to Visual reads the blocks of the buffer and marks the ones that differ from the saved file", async () => {
    const editor = await startSource();
    editor.text = "# A\n\nfirst, changed in Source\n\nsecond\n";
    editor.line = 2;
    edit.noteChange(editor.text.length);
    expect(await edit.switchTo("visual")).toBe(true);
    expect(fake.parseBlocks).toHaveBeenCalledWith("# A\n\nfirst, changed in Source\n\nsecond\n");
    expect(edit.mode).toBe("visual");
    expect(edit.initial).toBeNull();
    const start = edit.visualStart;
    expect(start?.map.blocks.map((b) => b.text)).toEqual([
      "# A",
      "first, changed in Source",
      "second",
    ]);
    expect(start?.unsaved).toEqual([false, true, false]);
    expect(start?.dirty).toBe(true);
    expect(start?.block).toBe(1);
  });

  it("Source to Visual with the text as saved starts clean", async () => {
    await startSource();
    await edit.switchTo("visual");
    expect(edit.visualStart?.dirty).toBe(false);
    expect(edit.visualStart?.unsaved).toEqual([false, false, false]);
  });

  it("ignores a second switch while the blocks of the first are being read", async () => {
    await startSource();
    const blocks = deferredBlocks();
    const first = edit.switchTo("visual");
    expect(edit.switching).toBe(true);
    expect(await edit.switchTo("visual")).toBe(false);
    expect(await edit.switchTo("source")).toBe(false);
    blocks.release(mockParseBlocks(ORIGINAL));
    expect(await first).toBe(true);
    expect(edit.switching).toBe(false);
    expect(edit.mode).toBe("visual");
  });

  it("drops a switch that the end of the session overtook", async () => {
    await startSource();
    const blocks = deferredBlocks();
    const pending = edit.switchTo("visual");
    edit.reset();
    blocks.release(mockParseBlocks(ORIGINAL));
    expect(await pending).toBe(false);
    expect(edit.visualStart).toBeNull();
    expect(edit.active).toBe(false);
  });

  it("reads the blocks again when the person typed while they were being read", async () => {
    const editor = await startSource();
    const blocks = deferredBlocks();
    const pending = edit.switchTo("visual");
    editor.text = `${ORIGINAL}\ntyped meanwhile\n`;
    blocks.release(mockParseBlocks(ORIGINAL));
    await pending;
    expect(edit.visualStart?.map.blocks.map((b) => b.text)).toContain("typed meanwhile");
  });

  it("stays where it is, and says why, when the blocks cannot be read", async () => {
    await startSource();
    fake.parseBlocks.mockRejectedValueOnce(new Error("backend down"));
    const warn = vi.spyOn(toast, "message");
    expect(await edit.switchTo("visual")).toBe(false);
    expect(edit.mode).toBe("source");
    expect(edit.switching).toBe(false);
    expect(warn).toHaveBeenCalled();
  });

  it("does nothing without an editor on screen, or to the mode it is already in", async () => {
    await startVisual();
    bridge.editor = undefined;
    expect(await edit.switchTo("source")).toBe(false);
    bridge.editor = fakeVisual(ORIGINAL);
    expect(await edit.switchTo("visual")).toBe(false);
  });

  it("falls back to Source with the saved text when the visual editor cannot be loaded", async () => {
    await startVisual();
    edit.visualUnavailable();
    expect(edit.mode).toBe("source");
    expect(edit.initial?.buffer).toBe(ORIGINAL);
    expect(edit.dirty).toBe(false);
  });

  it("falls back to Source with the unsaved text, and still unsaved, when the visual editor cannot be loaded after a switch", async () => {
    const source = await startSource();
    source.text = EDITED_IN_SOURCE;
    edit.noteChange(source.text.length);
    await edit.switchTo("visual");
    // the visual editor never mounts
    bridge.editor = undefined;
    edit.visualUnavailable();
    expect(edit.mode).toBe("source");
    expect(edit.initial?.buffer).toBe(EDITED_IN_SOURCE);
    expect(edit.dirty).toBe(true);
  });

  it("starts the Source editor on the line of the block the Visual editor was to open on", async () => {
    const source = await startSource();
    source.text = EDITED_IN_SOURCE;
    source.line = 2;
    edit.noteChange(source.text.length);
    await edit.switchTo("visual");
    bridge.editor = undefined;
    edit.visualUnavailable();
    expect(edit.initial?.line).toBe(2);
  });

  it("saves the text of the document the visual editor is about to open, when a save comes before it has loaded", async () => {
    const source = await startSource();
    source.text = EDITED_IN_SOURCE;
    edit.noteChange(source.text.length);
    await edit.switchTo("visual");
    // the editor's code is still loading: nothing is mounted
    bridge.editor = undefined;
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    expect(await edit.save()).toBe(true);
    expect(fake.saveDocument).toHaveBeenCalledWith(PATH, EDITED_IN_SOURCE, "h1");
    // it opens on what was saved, and clean
    expect(edit.dirty).toBe(false);
    expect(edit.visualStart?.dirty).toBe(false);
    expect(edit.visualStart?.unsaved).toBeUndefined();
    expect(edit.visualStart?.map.blocks.map((b) => b.text)).toEqual([
      "# A",
      "first, changed in Source",
      "second",
    ]);
  });

  it("does not take a document that is still being typed in for a finished one", async () => {
    const source = await startSource();
    source.text = EDITED_IN_SOURCE;
    edit.noteChange(source.text.length);
    let calls = 0;
    // every read of the blocks is followed by more typing
    fake.parseBlocks.mockImplementation(async (text: string): Promise<ParsedBlocks> => {
      calls++;
      source.text = `${source.text}.`;
      return mockParseBlocks(text);
    });
    const warn = vi.spyOn(toast, "message");
    expect(await edit.switchTo("visual")).toBe(false);
    expect(calls).toBeGreaterThan(1);
    expect(edit.mode).toBe("source");
    expect(edit.visualStart).toBeNull();
    expect(edit.switching).toBe(false);
    expect(warn).toHaveBeenCalled();
  });

  it("compares with the version saved while the blocks were being read, not the one from before", async () => {
    const source = await startSource();
    source.text = EDITED_IN_SOURCE;
    edit.noteChange(source.text.length);
    const blocks = deferredBlocks();
    const pending = edit.switchTo("visual");
    // a save lands while the blocks are being read
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    expect(await edit.save()).toBe(true);
    blocks.release(mockParseBlocks(EDITED_IN_SOURCE));
    expect(await pending).toBe(true);
    // the text is the saved one: no block is unsaved, and the document is not dirty
    expect(edit.visualStart?.dirty).toBe(false);
    expect(edit.visualStart?.unsaved).toEqual([false, false, false]);
  });
});

describe("images in Visual mode", () => {
  it("are linked through the editor's own insert", async () => {
    const editor = await startVisual();
    fake.writeAsset.mockResolvedValue({ path: "docs/assets/x.png", rel: "assets/x.png", bytes: 4 });
    await edit.addFiles([new File(["abcd"], "x.png", { type: "image/png" })], "drop");
    expect(editor.insertImage).toHaveBeenCalledWith("assets/x.png");
  });
});

describe("files moved or renamed in the Explorer", () => {
  const MOVED = "manual/a.md";
  const moves = [{ from: "docs", to: "manual" }];
  const edits = [{ from: 9, to: 15, insert: "new/old.md" }];

  beforeEach(() => {
    vi.useFakeTimers();
    return () => vi.useRealTimers();
  });

  it("holds back a document with unsaved edits in Visual mode and hands over no text for it", async () => {
    const editor = await startVisual();
    expect(edit.unsavedUnpatchable()).toBeNull();
    expect(edit.unsavedBuffer()).toBeNull();
    editor.dirty = true;
    edit.noteVisual();
    expect(edit.unsavedUnpatchable()).toBe(PATH);
    expect(edit.unsavedBuffer()).toBeNull();
    // back to the saved text: nothing to hold back
    editor.dirty = false;
    edit.noteVisual();
    expect(edit.unsavedUnpatchable()).toBeNull();
  });

  it("holds back a document with unsaved edits whose editor is not on screen yet", async () => {
    const editor = await startVisual();
    editor.dirty = true;
    edit.noteVisual();
    bridge.editor = undefined;
    expect(edit.unsavedUnpatchable()).toBe(PATH);
    expect(edit.unsavedBuffer()).toBeNull();
  });

  it("does not hold back a document with unsaved edits in Source mode: its text is patched", async () => {
    const editor = await startSource();
    expect(edit.unsavedUnpatchable()).toBeNull();
    editor.text = `${ORIGINAL}more\n`;
    edit.noteChange(editor.text.length);
    expect(edit.unsavedUnpatchable()).toBeNull();
    expect(edit.unsavedBuffer()).toEqual({ path: PATH, text: `${ORIGINAL}more\n` });
  });

  it("never patches the Visual editor with link changes, and keeps the document and its edits", async () => {
    const editor = await startVisual();
    editor.dirty = true;
    edit.noteVisual();
    const applied = edit.followMoves(moves, { path: PATH, newPath: MOVED, edits }, editor.current);
    expect(applied).toBe(false);
    expect(editor.loads).toHaveLength(0);
    expect(editor.rebased).toHaveLength(0);
    expect(edit.path).toBe(MOVED);
    expect(edit.dirty).toBe(true);
    // the file moved with its content: the check finds the version it started from, no conflict
    fake.documentSource.mockResolvedValue({ ...source(ORIGINAL, "h1"), path: MOVED });
    await vi.advanceTimersByTimeAsync(400);
    expect(fake.documentSource).toHaveBeenCalledWith(MOVED);
    expect(edit.conflict).toBeNull();
    expect(editor.loads).toHaveLength(0);
  });

  it("a clean Visual document takes in the links rewritten on disk", async () => {
    const editor = await startVisual();
    expect(edit.followMoves(moves)).toBe(true);
    fake.documentSource.mockResolvedValue({
      ...source("# A\n\nrewritten\n\nsecond\n", "h2"),
      path: MOVED,
    });
    await vi.advanceTimersByTimeAsync(400);
    expect(editor.loads).toHaveLength(1);
    expect(edit.dirty).toBe(false);
    expect(edit.conflict).toBeNull();
  });
});
