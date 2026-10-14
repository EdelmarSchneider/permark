import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../bridge";
import type { DocumentView, FsBatch, ParsedBlocks, SaveOutcome, SourceDoc } from "../types";

// The backend and the native dialogs are replaced: these tests are about what the store decides.
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
import { mockParseBlocks } from "../mock/mockBlocks";
import { askUnsaved, checkForUpdate } from "../platform";
import { INITIAL_UPDATE_STATE } from "../updates";
import { docs } from "./docs.svelte";
import { session } from "./session.svelte";
import { edit } from "./edit.svelte";
import { toast } from "./toast.svelte";
import { updates } from "./updates.svelte";

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

/** An editor that is only a string. */
function fakeEditor(initial: string): EditorApi & { text: string; line: number } {
  const editor = {
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
    insertImage: (rel: string) => {
      editor.text += `![](${rel})`;
    },
  };
  return editor;
}

/** Types into the fake editor the way CodeMirror reports it. */
function type(editor: ReturnType<typeof fakeEditor>, text: string): void {
  editor.text = text;
  edit.noteChange(text.length);
}

const PATH = "docs/a.md";
const ORIGINAL = "# A\n\nfirst\n\nsecond\n";

async function startEditing(text = ORIGINAL, hash = "h1"): Promise<ReturnType<typeof fakeEditor>> {
  docs.current = view(PATH);
  fake.documentSource.mockResolvedValue(source(PATH, text, hash));
  expect(await edit.enter()).toBe(true);
  const editor = fakeEditor(edit.initial?.buffer ?? "");
  bridge.editor = editor;
  return editor;
}

beforeEach(() => {
  session.editMode = "source";
  edit.reset();
  docs.reset();
  docs.guard = null;
  bridge.editor = undefined;
  for (const mock of Object.values(fake)) mock.mockReset();
  fake.parseBlocks.mockImplementation(async (text: string): Promise<ParsedBlocks> =>
    mockParseBlocks(text),
  );
  // the store's own constructor set the guard once; tests that need it set it again
  docs.guard = (resume) => edit.holdNavigation(resume);
});

describe("entering and the dirty state", () => {
  it("starts from the file's text and is clean until the text changes", async () => {
    const editor = await startEditing();
    expect(edit.active).toBe(true);
    expect(docs.editing).toBe(true);
    expect(edit.dirty).toBe(false);
    type(editor, `${ORIGINAL}more\n`);
    expect(edit.dirty).toBe(true);
    type(editor, ORIGINAL);
    expect(edit.dirty).toBe(false);
  });

  it("tells a change that keeps the length from no change at all", async () => {
    const editor = await startEditing();
    type(editor, ORIGINAL.replace("first", "FIRST"));
    expect(edit.dirty).toBe(true);
  });

  it("opens a file with a BOM and CRLF as an editor buffer without them", async () => {
    await startEditing("\uFEFF# A\r\n\r\nbody\r\n");
    expect(edit.initial?.buffer).toBe("# A\n\nbody\n");
  });

  it("refuses a document that cannot be edited, with a reason", async () => {
    docs.current = { ...view("a.json"), kind: "json" };
    expect(await edit.enter()).toBe(false);
    expect(edit.active).toBe(false);
    expect(fake.documentSource).not.toHaveBeenCalled();
    expect(edit.blocked).toMatch(/Only Markdown/);
  });
});

describe("saving", () => {
  it("writes the text with the file's own line endings and the hash it started from", async () => {
    const editor = await startEditing("\uFEFF# A\r\n\r\nbody\r\n", "h1");
    type(editor, "# A\n\nbody changed\n");
    fake.saveDocument.mockResolvedValue({
      result: "saved",
      hash: "h2",
      modifiedMs: 200,
    } satisfies SaveOutcome);
    expect(await edit.save()).toBe(true);
    expect(fake.saveDocument).toHaveBeenCalledWith(PATH, "\uFEFF# A\r\n\r\nbody changed\r\n", "h1");
    expect(edit.dirty).toBe(false);
    expect(edit.note).toBe("Saved, 1 block changed");
    expect(edit.modifiedMs).toBe(200);
  });

  it("writes a file with mixed line endings back with the line endings of the lines it did not touch", async () => {
    const mixed = "# A\r\n\r\nfirst\nsecond\r\n";
    const editor = await startEditing(mixed, "h1");
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    await edit.save(); // no edit at all: the very same bytes
    expect(fake.saveDocument).toHaveBeenLastCalledWith(PATH, mixed, "h1");
    type(editor, "# B\n\nfirst\nsecond\n");
    await edit.save();
    expect(fake.saveDocument).toHaveBeenLastCalledWith(PATH, "# B\r\n\r\nfirst\nsecond\r\n", "h2");
  });

  it("keeps a file with lone CR line endings as it is", async () => {
    const cr = "# T\r\rbody\r";
    const editor = await startEditing(cr, "h1");
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    await edit.save();
    expect(fake.saveDocument).toHaveBeenLastCalledWith(PATH, cr, "h1");
    type(editor, "# T\n\nbody!\n");
    await edit.save();
    expect(fake.saveDocument).toHaveBeenLastCalledWith(PATH, "# T\r\rbody!\r", "h2");
  });

  it("goes on from the saved version: the next save expects the new hash", async () => {
    const editor = await startEditing();
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    type(editor, `${ORIGINAL}x\n`);
    await edit.save();
    type(editor, `${ORIGINAL}xy\n`);
    await edit.save();
    expect(fake.saveDocument.mock.calls[1][2]).toBe("h2");
  });

  it("stays dirty when the person typed while the save was running", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    fake.saveDocument.mockImplementation(async () => {
      type(editor, `${ORIGINAL}x and more\n`);
      return { result: "saved", hash: "h2", modifiedMs: 200 };
    });
    await edit.save();
    expect(edit.dirty).toBe(true);
  });

  it("keeps the buffer and shows the error when the file cannot be written", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    fake.saveDocument.mockRejectedValue({
      kind: "denied",
      message: "cannot write docs/a.md: Access is denied.",
    });
    expect(await edit.save()).toBe(false);
    expect(edit.saveError).toMatch(/Access is denied/);
    expect(edit.dirty).toBe(true);
    expect(editor.text).toBe(`${ORIGINAL}x\n`);
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    edit.retry();
    await vi.waitFor(() => expect(edit.dirty).toBe(false));
    expect(edit.saveError).toBeNull();
  });

  it("shows the one-time hint on the first save only", async () => {
    Reflect.set(edit, "hintSeen", false); // as on a first launch
    const editor = await startEditing();
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    type(editor, `${ORIGINAL}x\n`);
    await edit.save();
    expect(edit.hint).toBe(true);
    edit.dismissHint();
    type(editor, `${ORIGINAL}xy\n`);
    await edit.save();
    expect(edit.hint).toBe(false);
  });
});

describe("the file changed on disk", () => {
  const changed: FsBatch = {
    changes: [{ kind: "modified", path: PATH, atMs: 1 }],
    bulk: null,
    rescan: false,
  };

  beforeEach(() => {
    vi.useFakeTimers();
    return () => vi.useRealTimers();
  });

  it("reloads a clean buffer and keeps the caret on its block", async () => {
    const editor = await startEditing();
    editor.line = 4; // "second"
    fake.documentSource.mockResolvedValue(
      source(PATH, "# A\n\nnew block\n\nfirst\n\nsecond\n", "h2"),
    );
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    expect(editor.text).toBe("# A\n\nnew block\n\nfirst\n\nsecond\n");
    expect(editor.line).toBe(6); // still on "second"
    expect(edit.dirty).toBe(false);
    expect(edit.conflict).toBeNull();
    expect(edit.note).toMatch(/^Reloaded from disk \d\d:\d\d$/);
  });

  it("ignores a change whose content is the version being edited (our own save)", async () => {
    const editor = await startEditing();
    editor.line = 2;
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    expect(editor.text).toBe(ORIGINAL);
    expect(edit.note).toBeNull();
  });

  it("does not put the version from before a save back into the editor", async () => {
    const editor = await startEditing();
    // a check starts and reads the file as it was before the save ...
    let arrive!: (doc: SourceDoc) => void;
    fake.documentSource.mockImplementationOnce(
      () =>
        new Promise<SourceDoc>((resolve) => {
          arrive = resolve;
        }),
    );
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    // ... the person saves while it is reading ...
    type(editor, `${ORIGINAL}mine\n`);
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    expect(await edit.save()).toBe(true);
    // ... and the old read arrives
    fake.documentSource.mockResolvedValue(source(PATH, `${ORIGINAL}mine\n`, "h2"));
    arrive(source(PATH, ORIGINAL, "h1"));
    await vi.advanceTimersByTimeAsync(50);
    expect(editor.text).toBe(`${ORIGINAL}mine\n`);
    expect(edit.note).not.toMatch(/Reloaded/);
    expect(edit.conflict).toBeNull();
    expect(edit.dirty).toBe(false);
    // the file was looked at again, and was our own version
    expect(fake.documentSource).toHaveBeenCalledTimes(3);
  });

  it("waits for a save that is running before it looks at the file", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}mine\n`);
    let finish!: (outcome: SaveOutcome) => void;
    fake.saveDocument.mockImplementation(
      () =>
        new Promise<SaveOutcome>((resolve) => {
          finish = resolve;
        }),
    );
    const saving = edit.save();
    fake.documentSource.mockClear();
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    expect(fake.documentSource).not.toHaveBeenCalled();
    fake.documentSource.mockResolvedValue(source(PATH, `${ORIGINAL}mine\n`, "h2"));
    finish({ result: "saved", hash: "h2", modifiedMs: 200 });
    expect(await saving).toBe(true);
    await vi.advanceTimersByTimeAsync(50);
    expect(fake.documentSource).toHaveBeenCalledTimes(1);
    expect(edit.conflict).toBeNull();
    expect(edit.note).not.toMatch(/Reloaded/);
  });

  it("ignores changes to other files", async () => {
    await startEditing();
    fake.documentSource.mockClear();
    edit.onFsChange({
      changes: [{ kind: "modified", path: "other.md", atMs: 1 }],
      bulk: null,
      rescan: false,
    });
    await vi.advanceTimersByTimeAsync(400);
    expect(fake.documentSource).not.toHaveBeenCalled();
  });

  it("leaves a dirty buffer alone and raises the conflict", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}mine\n`);
    fake.documentSource.mockResolvedValue(source(PATH, "# A\n\ntheirs\n", "h2"));
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    expect(editor.text).toBe(`${ORIGINAL}mine\n`);
    expect(edit.conflict).toMatchObject({ deleted: false, diskHash: "h2" });
  });

  it("with a conflict, Ctrl+S writes nothing and moves the focus to the bar", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}mine\n`);
    fake.documentSource.mockResolvedValue(source(PATH, "# A\n\ntheirs\n", "h2"));
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    const before = edit.conflictFocus;
    expect(await edit.save()).toBe(false);
    expect(fake.saveDocument).not.toHaveBeenCalled();
    expect(edit.conflictFocus).toBe(before + 1);
  });

  it("Keep mine saves over the version on disk by naming it as the one it is based on", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}mine\n`);
    fake.documentSource.mockResolvedValue(source(PATH, "# A\n\ntheirs\n", "h2"));
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h3", modifiedMs: 300 });
    await edit.keepMine();
    expect(fake.saveDocument).toHaveBeenCalledWith(PATH, `${ORIGINAL}mine\n`, "h2");
    expect(edit.conflict).toBeNull();
    expect(edit.dirty).toBe(false);
  });

  it("Reload theirs takes the file's version and drops the edits", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}mine\n`);
    fake.documentSource.mockResolvedValue(source(PATH, "# A\n\ntheirs\n", "h2"));
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    await edit.reloadTheirs();
    expect(editor.text).toBe("# A\n\ntheirs\n");
    expect(edit.dirty).toBe(false);
    expect(edit.conflict).toBeNull();
  });

  it("asks before Reload theirs throws away edits in more than five blocks", async () => {
    const many = Array.from({ length: 8 }, (_, i) => `block ${i}`).join("\n\n");
    const editor = await startEditing(`${many}\n`);
    type(editor, `${many.replace(/block/g, "BLOCK")}\n`);
    fake.documentSource.mockResolvedValue(source(PATH, "# theirs\n", "h2"));
    edit.onFsChange(changed);
    await vi.advanceTimersByTimeAsync(400);
    await edit.reloadTheirs();
    expect(edit.conflict?.confirmReload).toBe(8);
    expect(editor.text).toContain("BLOCK");
    edit.cancelReload();
    expect(edit.conflict?.confirmReload).toBeNull();
    await edit.reloadTheirs(true);
    expect(editor.text).toBe("# theirs\n");
  });

  it("a file deleted on disk is a conflict that offers to write it again", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}mine\n`);
    fake.documentSource.mockRejectedValue({ kind: "notFound", message: "not found" });
    edit.onFsChange({
      changes: [{ kind: "removed", path: PATH, atMs: 1 }],
      bulk: null,
      rescan: false,
    });
    await vi.advanceTimersByTimeAsync(400);
    expect(edit.conflict).toMatchObject({ deleted: true, diskHash: null });
    fake.createDocument.mockResolvedValue({ path: PATH, hash: "h9", modifiedMs: 500 });
    await edit.saveAsNew();
    expect(fake.createDocument).toHaveBeenCalledWith(PATH, `${ORIGINAL}mine\n`);
    expect(edit.conflict).toBeNull();
    expect(edit.dirty).toBe(false);
  });

  it("a save that finds the file changed is a conflict, and writes nothing", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}mine\n`);
    fake.saveDocument.mockResolvedValue({ result: "conflict", diskHash: "h7" });
    expect(await edit.save()).toBe(false);
    expect(edit.conflict).toMatchObject({ deleted: false, diskHash: "h7" });
    expect(edit.dirty).toBe(true);
  });
});

describe("leaving", () => {
  it("a clean exit goes straight back to reading", async () => {
    await startEditing();
    edit.requestExit();
    await vi.waitFor(() => expect(edit.active).toBe(false));
    expect(docs.editing).toBe(false);
    expect(edit.prompt).toBeNull();
  });

  it("an exit with unsaved work asks first, and Keep editing stays", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    edit.requestExit();
    expect(edit.prompt?.reason).toBe("exit");
    expect(edit.active).toBe(true);
    edit.keepEditing();
    expect(edit.prompt).toBeNull();
    expect(edit.active).toBe(true);
  });

  it("Save on the bar writes, then leaves", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    fake.readDocument.mockResolvedValue(view(PATH));
    edit.requestExit();
    await edit.promptSave();
    await vi.waitFor(() => expect(edit.active).toBe(false));
    expect(fake.saveDocument).toHaveBeenCalledTimes(1);
  });

  it("Save on the bar does not leave when the save fails", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    fake.saveDocument.mockRejectedValue(new Error("disk full"));
    edit.requestExit();
    await edit.promptSave();
    expect(edit.active).toBe(true);
    expect(edit.saveError).toBe("disk full");
  });

  it("navigating away while dirty holds the move, and Discard carries it on", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    const resume = vi.fn();
    expect(edit.holdNavigation(resume)).toBe(true);
    expect(edit.prompt?.reason).toBe("navigate");
    expect(resume).not.toHaveBeenCalled();
    edit.promptDiscard();
    await vi.waitFor(() => expect(resume).toHaveBeenCalledTimes(1));
    expect(edit.active).toBe(false);
    expect(fake.saveDocument).not.toHaveBeenCalled();
  });

  it("navigating away from a clean buffer just ends editing", async () => {
    await startEditing();
    const resume = vi.fn();
    expect(edit.holdNavigation(resume)).toBe(false);
    expect(edit.active).toBe(false);
    expect(resume).not.toHaveBeenCalled();
  });

  it("docs.open holds a move to another document while dirty, and lets one to the same document through", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    expect(await docs.open("docs/b.md")).toBe(false);
    expect(edit.prompt?.reason).toBe("navigate");
    expect(docs.path).toBe(PATH);
  });
});

describe("restarting to update", () => {
  const install = vi.fn(async () => {});

  beforeEach(async () => {
    install.mockClear();
    vi.mocked(askUnsaved).mockReset();
    updates.state = INITIAL_UPDATE_STATE;
    vi.mocked(checkForUpdate).mockResolvedValue({
      version: "0.2.0",
      install,
      close: async () => {},
    });
    await updates.check(true);
    expect(updates.state.phase).toBe("available");
  });

  it("goes on at once when there is nothing unsaved", async () => {
    await startEditing();
    await updates.install();
    expect(askUnsaved).not.toHaveBeenCalled();
    expect(install).toHaveBeenCalledTimes(1);
  });

  it("asks first when there are unsaved edits, and Keep editing stops the update", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    vi.mocked(askUnsaved).mockResolvedValue("keep");
    await updates.install();
    expect(vi.mocked(askUnsaved).mock.calls[0][0]).toMatch(/restarting to update/);
    expect(install).not.toHaveBeenCalled();
    expect(updates.state.phase).toBe("available");
    expect(edit.dirty).toBe(true);
    expect(editor.text).toBe(`${ORIGINAL}x\n`);
  });

  it("Save writes the edits before the update starts", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    vi.mocked(askUnsaved).mockResolvedValue("save");
    fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
    await updates.install();
    expect(fake.saveDocument).toHaveBeenCalledTimes(1);
    expect(install).toHaveBeenCalledTimes(1);
  });

  it("does not start the update when the save did not work", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    vi.mocked(askUnsaved).mockResolvedValue("save");
    fake.saveDocument.mockRejectedValue(new Error("disk full"));
    await updates.install();
    expect(install).not.toHaveBeenCalled();
    expect(edit.saveError).toBe("disk full");
    expect(edit.dirty).toBe(true);
  });

  it("Discard lets the update go on", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    vi.mocked(askUnsaved).mockResolvedValue("discard");
    await updates.install();
    expect(fake.saveDocument).not.toHaveBeenCalled();
    expect(install).toHaveBeenCalledTimes(1);
  });
});

describe("a new file opened while another document has unsaved edits", () => {
  it("starts editing after the answer, as it does when nothing is held", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}x\n`);
    fake.readDocument.mockResolvedValue(view("docs/new.md"));
    fake.fileMeta.mockResolvedValue({ tags: [], description: null });
    const entered = vi.fn();
    expect(await docs.open("docs/new.md", { afterOpen: entered })).toBe(false);
    expect(edit.prompt?.reason).toBe("navigate");
    expect(entered).not.toHaveBeenCalled();
    edit.promptDiscard();
    await vi.waitFor(() => expect(entered).toHaveBeenCalledTimes(1));
    expect(docs.path).toBe("docs/new.md");
  });
});

describe("images", () => {
  it("refuses an image over the limit without reading it", async () => {
    const editor = await startEditing();
    const message = vi.spyOn(toast, "message");
    const file = new File([new Uint8Array([1])], "huge.png", { type: "image/png" });
    Object.defineProperty(file, "size", { value: 40 * 1024 * 1024 });
    const read = vi.spyOn(file, "arrayBuffer");
    await edit.addFiles([file], "drop");
    expect(read).not.toHaveBeenCalled();
    expect(fake.writeAsset).not.toHaveBeenCalled();
    expect(message.mock.calls[0][0]).toMatch(/huge\.png is 40\.0 MB, over the 32\.0 MB limit/);
    expect(editor.text).toBe(ORIGINAL);
    message.mockRestore();
  });

  it("writes a pasted picture next to the document and links it at the caret", async () => {
    const editor = await startEditing();
    fake.writeAsset.mockResolvedValue({
      path: "docs/assets/a.png",
      rel: "assets/a-20261014-104201.png",
      bytes: 3,
    });
    const file = new File([new Uint8Array([1, 2, 3])], "image.png", { type: "image/png" });
    await edit.addFiles([file], "paste");
    expect(fake.writeAsset).toHaveBeenCalledTimes(1);
    const [docPath, name, data] = fake.writeAsset.mock.calls[0];
    expect(docPath).toBe(PATH);
    expect(name).toMatch(/^a-\d{8}-\d{6}\.png$/);
    expect(data).toBe("AQID");
    expect(editor.text.endsWith("![](assets/a-20261014-104201.png)")).toBe(true);
  });

  it("keeps the name of a dropped file and refuses what is not an image", async () => {
    const editor = await startEditing();
    fake.writeAsset.mockResolvedValue({
      path: "docs/assets/shot.png",
      rel: "assets/shot.png",
      bytes: 1,
    });
    await edit.addFiles([new File(["x"], "My Shot.PNG", { type: "image/png" })], "drop");
    expect(fake.writeAsset.mock.calls[0][1]).toBe("My Shot.PNG");
    fake.writeAsset.mockClear();
    const before = editor.text;
    await edit.addFiles([new File(["x"], "notes.txt", { type: "text/plain" })], "drop");
    expect(fake.writeAsset).not.toHaveBeenCalled();
    expect(editor.text).toBe(before);
  });
});
