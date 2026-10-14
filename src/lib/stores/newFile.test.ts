import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentView, ParsedBlocks, SourceDoc } from "../types";

// The backend and the native dialogs are replaced: these tests are about what Ctrl+N decides.
const fake = vi.hoisted(() => ({
  documentSource: vi.fn(),
  parseBlocks: vi.fn(),
  saveDocument: vi.fn(),
  createDocument: vi.fn(),
  readDocument: vi.fn(),
  listTree: vi.fn(),
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
  watchFileDrop: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));

import { bridge } from "../bridge";
import { noOutline } from "../editor/outlineStub";
import { mockParseBlocks } from "../mock/mockBlocks";
import { app } from "./app.svelte";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";
import { vault } from "./vault.svelte";

const PATH = "docs/a.md";
const ORIGINAL = "# A\n\nfirst\n";

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

const source = (path: string, text: string): SourceDoc => ({
  path,
  text,
  hash: "h1",
  modifiedMs: 100,
  ...mockParseBlocks(text),
});

/** An editor that is only a string. */
function fakeEditor(initial: string) {
  const editor = {
    ...noOutline,
    mode: "source" as const,
    text: initial,
    getText: () => editor.text,
    setText: (text: string) => {
      editor.text = text;
    },
    caretLine: () => 0,
    focus: vi.fn(),
    insertImage: () => {},
    applyChanges: () => {},
  };
  return editor;
}

async function startEditing() {
  docs.current = view(PATH);
  fake.documentSource.mockResolvedValue(source(PATH, ORIGINAL));
  expect(await edit.enter()).toBe(true);
  const editor = fakeEditor(edit.initial?.buffer ?? "");
  bridge.editor = editor;
  return editor;
}

function type(editor: ReturnType<typeof fakeEditor>, text: string): void {
  editor.text = text;
  edit.noteChange(text.length);
}

beforeEach(() => {
  session.editMode = "source";
  edit.reset();
  docs.reset();
  ui.newFile = null;
  toast.clear?.();
  bridge.editor = undefined;
  for (const mock of Object.values(fake)) mock.mockReset();
  fake.parseBlocks.mockImplementation(async (text: string): Promise<ParsedBlocks> =>
    mockParseBlocks(text),
  );
  fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
  fake.listTree.mockResolvedValue({ name: "", path: "", kind: "dir", children: [] });
  docs.guard = (resume) => edit.holdNavigation(resume);
  vault.info = { mode: "project", root: "/p", name: "p", worktrees: [] } as never;
});

describe("Ctrl+N (new file)", () => {
  it("asks for the name at once when nothing is being edited", () => {
    app.startNewFile();
    expect(ui.newFile).not.toBeNull();
    expect(edit.prompt).toBeNull();
  });

  it("asks for the name at once while editing a document with nothing unsaved, and goes on editing", async () => {
    await startEditing();
    app.startNewFile();
    expect(ui.newFile).not.toBeNull();
    expect(edit.prompt).toBeNull();
    expect(edit.active).toBe(true);
  });

  it("asks Save, Discard or Keep editing first when the open document has unsaved edits", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}more\n`);
    app.startNewFile();
    expect(edit.prompt).not.toBeNull();
    expect(ui.newFile).toBeNull();
    expect(fake.createDocument).not.toHaveBeenCalled();
  });

  it("Keep editing creates nothing and leaves the edits where they are", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}more\n`);
    app.startNewFile();
    edit.keepEditing();
    expect(edit.prompt).toBeNull();
    expect(ui.newFile).toBeNull();
    expect(edit.active).toBe(true);
    expect(edit.dirty).toBe(true);
    expect(editor.text).toBe(`${ORIGINAL}more\n`);
    expect(fake.createDocument).not.toHaveBeenCalled();
    expect(fake.saveDocument).not.toHaveBeenCalled();
  });

  it("Discard drops the edits, then asks for the name", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}more\n`);
    app.startNewFile();
    edit.promptDiscard();
    await vi.waitFor(() => expect(ui.newFile).not.toBeNull());
    expect(edit.active).toBe(false);
    expect(fake.saveDocument).not.toHaveBeenCalled();
    expect(fake.createDocument).not.toHaveBeenCalled();
  });

  it("Save writes the edits, then asks for the name", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}more\n`);
    app.startNewFile();
    await edit.promptSave();
    await vi.waitFor(() => expect(ui.newFile).not.toBeNull());
    expect(fake.saveDocument).toHaveBeenCalledTimes(1);
    expect(edit.active).toBe(false);
    expect(fake.createDocument).not.toHaveBeenCalled();
  });

  it("asks in the folder it was started for, after the answer", async () => {
    const editor = await startEditing();
    type(editor, `${ORIGINAL}more\n`);
    app.startNewFile("docs");
    edit.promptDiscard();
    await vi.waitFor(() => expect(ui.newFile?.folder).toBe("docs"));
  });
});
