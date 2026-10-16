import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  CreatedFolder,
  DeleteOutcome,
  DocumentView,
  Move,
  MoveOutcome,
  MovePreview,
  SkippedFile,
  TreeNode,
  UndoOutcome,
} from "../types";

// The backend and the native dialogs are replaced: these tests are about what the store decides.
const fake = vi.hoisted(() => ({
  previewMove: vi.fn(),
  applyMove: vi.fn(),
  deleteEntries: vi.fn(),
  newFolder: vi.fn(),
  undoFileOp: vi.fn(),
  listTree: vi.fn(),
  readDocument: vi.fn(),
  documentSource: vi.fn(),
  fileMeta: vi.fn(),
  askYesNo: vi.fn(),
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
  askYesNo: fake.askYesNo,
  copyText: vi.fn(async () => true),
  guardWindowClose: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));

import { bridge } from "../bridge";
import { noOutline } from "../editor/outlineStub";
import { skipNotes } from "../fileOps";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { feed } from "./feed.svelte";
import { fileops } from "./fileops.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { vault } from "./vault.svelte";

const file = (path: string): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "file",
});
const dir = (path: string, children: TreeNode[]): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "dir",
  children,
});
const TREE: TreeNode = dir("", [
  dir("docs", [file("docs/guide.md"), file("docs/api.md")]),
  dir("archive", []),
  file("README.md"),
]);

const preview = (linkCount: number, moves: Move[]): MovePreview => ({
  planId: 7,
  moves,
  linkCount,
  fileCount: linkCount > 0 ? 1 : 0,
  files: [],
  truncated: false,
  skipped: [],
});

const outcome = (moved: Move[], extra: Partial<MoveOutcome> = {}): MoveOutcome => ({
  opId: 11,
  moved,
  filesUpdated: 0,
  linksUpdated: 0,
  buffers: [],
  skipped: [],
  ...extra,
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
  hash: null,
  encodingNote: null,
});

const RENAME: Move[] = [{ from: "docs/guide.md", to: "docs/manual.md" }];

/** Lets the store ask its question, then answers it. */
async function answer(choice: "apply" | "moveOnly" | "cancel"): Promise<void> {
  await vi.waitFor(() => expect(fileops.prompt).not.toBeNull());
  fileops.answer(choice);
}

beforeEach(() => {
  fileops.reset();
  edit.reset();
  docs.reset();
  docs.guard = null;
  feed.clear();
  toast.clear();
  vault.reset();
  bridge.editor = undefined;
  for (const mock of Object.values(fake)) mock.mockReset();
  fake.listTree.mockResolvedValue(TREE);
  fake.fileMeta.mockResolvedValue({ path: "", tags: [], description: "", writable: true });
  fake.askYesNo.mockResolvedValue(true);
  vault.tree = TREE;
  vault.info = {
    root: "/p",
    name: "p",
    kind: "project",
    mode: "ide",
    initialFile: null,
    worktrees: [],
    liveUpdates: true,
    treeDefault: null,
  };
  session.setLinkUpdate("ask");
});

describe("renaming and moving", () => {
  it("asks about the links, and Apply moves and rewrites them", async () => {
    fake.previewMove.mockResolvedValue(preview(3, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME, { filesUpdated: 2, linksUpdated: 3 }));
    const done = fileops.rename("docs/guide.md", "manual.md");
    await answer("apply");
    expect(await done).toBeNull();
    expect(fake.previewMove).toHaveBeenCalledWith(RENAME, true, []);
    expect(fake.applyMove).toHaveBeenCalledWith(7, true, []);
    expect(fileops.renaming).toBeNull();
    // one row in the feed, one toast with Undo
    expect(feed.entries).toHaveLength(1);
    expect(feed.entries[0]).toMatchObject({
      type: "op",
      kind: "renamed",
      summary: "Renamed guide.md to manual.md, 3 links updated",
    });
    expect(toast.current).toMatchObject({
      type: "message",
      text: "Renamed guide.md to manual.md, 3 links updated",
      action: { label: "Undo" },
    });
  });

  it("Move without updating moves and leaves the links", async () => {
    fake.previewMove.mockResolvedValue(preview(3, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME));
    const done = fileops.rename("docs/guide.md", "manual.md");
    await answer("moveOnly");
    await done;
    expect(fake.applyMove).toHaveBeenCalledWith(7, false, []);
    expect(feed.entries[0]).toMatchObject({ summary: "Renamed guide.md to manual.md" });
  });

  it("Cancel does nothing at all", async () => {
    fake.previewMove.mockResolvedValue(preview(3, RENAME));
    const done = fileops.rename("docs/guide.md", "manual.md");
    await answer("cancel");
    expect(await done).toBeNull();
    expect(fake.applyMove).not.toHaveBeenCalled();
    expect(feed.entries).toHaveLength(0);
    expect(toast.current).toBeNull();
  });

  it("does not ask when the move changes no link", async () => {
    fake.previewMove.mockResolvedValue(preview(0, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME));
    await fileops.rename("docs/guide.md", "manual.md");
    expect(fileops.prompt).toBeNull();
    expect(fake.applyMove).toHaveBeenCalledWith(7, true, []);
  });

  it("Always updates the links without asking", async () => {
    session.setLinkUpdate("always");
    fake.previewMove.mockResolvedValue(preview(5, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME, { filesUpdated: 1, linksUpdated: 5 }));
    await fileops.rename("docs/guide.md", "manual.md");
    expect(fake.previewMove).toHaveBeenCalledWith(RENAME, true, []);
    expect(fake.applyMove).toHaveBeenCalledWith(7, true, []);
  });

  it("Never moves without looking at the documents or asking", async () => {
    session.setLinkUpdate("never");
    fake.previewMove.mockResolvedValue(preview(0, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME));
    await fileops.rename("docs/guide.md", "manual.md");
    expect(fake.previewMove).toHaveBeenCalledWith(RENAME, false, []);
    expect(fake.applyMove).toHaveBeenCalledWith(7, false, []);
  });

  it("a rename to the same name is nothing", async () => {
    expect(await fileops.rename("docs/guide.md", "guide.md")).toBeNull();
    expect(fake.previewMove).not.toHaveBeenCalled();
  });

  it("hands back the reason a rename cannot be done, for the name field, and keeps the field open", async () => {
    fileops.startRename("docs/guide.md");
    fake.previewMove.mockRejectedValue({ kind: "exists", message: "docs/api.md already exists" });
    expect(await fileops.rename("docs/guide.md", "api.md")).toBe("docs/api.md already exists");
    expect(fileops.renaming?.path).toBe("docs/guide.md");
    expect(fake.applyMove).not.toHaveBeenCalled();
  });

  it("moves entries into a folder, keeping their names, and says what went wrong in a toast", async () => {
    const moves = [
      { from: "docs/guide.md", to: "archive/guide.md" },
      { from: "README.md", to: "archive/README.md" },
    ];
    fake.previewMove.mockResolvedValue(preview(0, moves));
    fake.applyMove.mockResolvedValue(outcome(moves));
    await fileops.move(["docs/guide.md", "README.md"], "archive");
    expect(fake.previewMove).toHaveBeenCalledWith(moves, true, []);
    expect(feed.entries[0]).toMatchObject({ kind: "moved", summary: "Moved 2 files" });

    fake.previewMove.mockRejectedValue({ kind: "otherRoot", message: "in another root" });
    await fileops.move(["docs/api.md"], "archive");
    expect(toast.current).toMatchObject({
      tone: "warn",
      text: "Couldn't move: in another root",
    });
  });

  it("a folder and the files inside it move once, and a move to where it is does nothing", async () => {
    await fileops.move(["docs/guide.md"], "docs");
    expect(fake.previewMove).not.toHaveBeenCalled();
    const moves = [{ from: "docs", to: "archive/docs" }];
    fake.previewMove.mockResolvedValue(preview(0, moves));
    fake.applyMove.mockResolvedValue(outcome(moves));
    await fileops.move(["docs", "docs/api.md"], "archive");
    expect(fake.previewMove).toHaveBeenCalledWith(moves, true, []);
    expect(feed.entries[0]).toMatchObject({ summary: "Moved 1 folder" });
  });

  it("follows the move in the open document, the history and the open folders", async () => {
    docs.current = view("docs/guide.md");
    vault.expanded.add("docs");
    fake.readDocument.mockResolvedValue(view("docs/manual.md"));
    fake.previewMove.mockResolvedValue(preview(0, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME));
    await fileops.rename("docs/guide.md", "manual.md");
    expect(fake.readDocument).toHaveBeenCalledWith("docs/manual.md");
    expect(docs.path).toBe("docs/manual.md");
    expect(docs.deleted).toBeNull();

    const folder = [{ from: "docs", to: "manual" }];
    fake.previewMove.mockResolvedValue(preview(0, folder));
    fake.applyMove.mockResolvedValue(outcome(folder));
    fake.readDocument.mockResolvedValue(view("manual/manual.md"));
    await fileops.rename("docs", "manual");
    expect(vault.expanded.has("manual")).toBe(true);
    expect(vault.expanded.has("docs")).toBe(false);
  });

  it("gives the unsaved text of the document being edited, and applies the changes it gets back", async () => {
    docs.current = view("docs/guide.md");
    fake.documentSource.mockResolvedValue({
      path: "docs/guide.md",
      text: "[a](api.md)\n",
      hash: "h1",
      modifiedMs: 1,
      eol: "lf",
      blocks: [],
    });
    expect(await edit.enter()).toBe(true);
    const editor = {
      ...noOutline,
      mode: "source" as const,
      text: "[a](api.md) unsaved\n",
      getText: () => editor.text,
      setText: vi.fn(),
      caretLine: () => 0,
      topLine: () => 0,
      focus: vi.fn(),
      insertImage: vi.fn(),
      applyChanges: vi.fn((edits: { from: number; to: number; insert: string }[]) => {
        editor.text =
          editor.text.slice(0, edits[0].from) + edits[0].insert + editor.text.slice(edits[0].to);
      }),
    };
    bridge.editor = editor;
    edit.noteChange(editor.text.length);
    expect(edit.dirty).toBe(true);

    const moves = [{ from: "docs/api.md", to: "docs/reference.md" }];
    fake.previewMove.mockResolvedValue(preview(1, moves));
    fake.applyMove.mockResolvedValue(
      outcome(moves, {
        buffers: [
          {
            path: "docs/guide.md",
            newPath: "docs/guide.md",
            edits: [{ from: 4, to: 10, insert: "reference.md" }],
          },
        ],
      }),
    );
    fake.readDocument.mockResolvedValue(view("docs/guide.md"));
    session.setLinkUpdate("always");
    await fileops.rename("docs/api.md", "reference.md");
    // a Source buffer is patched, not held back
    expect(fake.previewMove).toHaveBeenCalledWith(moves, true, []);
    expect(fake.applyMove).toHaveBeenCalledWith(7, true, [
      { path: "docs/guide.md", text: "[a](api.md) unsaved\n" },
    ]);
    expect(editor.text).toBe("[a](reference.md) unsaved\n");
    // the link patched into the buffer counts like one written to disk (the question counted it too)
    expect(feed.entries[0]).toMatchObject({
      summary: "Renamed api.md to reference.md, 1 link updated",
    });
    expect(toast.current).toMatchObject({ text: "Renamed api.md to reference.md, 1 link updated" });
  });

  /** A Visual editor holding the open document, with or without unsaved edits. */
  async function openInVisual(dirty: boolean): Promise<void> {
    docs.current = view("docs/guide.md");
    fake.documentSource.mockResolvedValue({
      path: "docs/guide.md",
      text: "[a](api.md)\n",
      hash: "h1",
      modifiedMs: 1,
      eol: "lf",
      blocks: [],
    });
    expect(await edit.enter()).toBe(true);
    bridge.editor = {
      ...noOutline,
      mode: "visual",
      text: () => "[a](api.md)\n",
      status: () => ({ dirty, changed: dirty ? 1 : 0 }),
      caretLine: () => 0,
      topLine: () => 0,
      caretBlock: () => 0,
      lineOfBlock: () => 0,
      load: vi.fn(),
      rebase: vi.fn(),
      focus: vi.fn(),
      insertImage: vi.fn(),
    };
    edit.noteVisual();
    expect(edit.dirty).toBe(dirty);
  }

  const GUIDE_HELD: SkippedFile = { path: "docs/guide.md", reason: "unsavedVisual" };
  const GUIDE_NOTE =
    "guide.md: has unsaved edits in Visual mode, so its links were not updated; save it, then undo the move and move it again";
  const REFERENCE: Move[] = [{ from: "docs/api.md", to: "docs/reference.md" }];

  it("leaves a document with unsaved edits in Visual mode out of the rewrite, says so, and keeps the rest working", async () => {
    await openInVisual(true);
    const editor = bridge.editor;
    fake.previewMove.mockResolvedValue({ ...preview(1, REFERENCE), skipped: [GUIDE_HELD] });
    fake.applyMove.mockResolvedValue(
      outcome(REFERENCE, { filesUpdated: 1, linksUpdated: 1, skipped: [GUIDE_HELD] }),
    );
    fake.readDocument.mockResolvedValue(view("docs/guide.md"));
    session.setLinkUpdate("always");
    await fileops.rename("docs/api.md", "reference.md");

    // the backend is told to leave it out, and gets no text for it
    expect(fake.previewMove).toHaveBeenCalledWith(REFERENCE, true, ["docs/guide.md"]);
    expect(fake.applyMove).toHaveBeenCalledWith(7, true, []);
    // the move is made and reported, with the document named as not updated
    expect(toast.current).toMatchObject({
      tone: "warn",
      text: "Renamed api.md to reference.md, 1 link updated. 1 document wasn't updated",
    });
    expect(feed.entries[0]).toMatchObject({ notes: [GUIDE_NOTE] });
    // the editor and its edits are untouched
    expect(bridge.editor).toBe(editor);
    expect(edit.dirty).toBe(true);
    expect(edit.path).toBe("docs/guide.md");
    expect(edit.conflict).toBeNull();
  });

  it("lists the held-back document in the question about links", async () => {
    await openInVisual(true);
    fake.previewMove.mockResolvedValue({ ...preview(1, REFERENCE), skipped: [GUIDE_HELD] });
    fake.applyMove.mockResolvedValue(outcome(REFERENCE, { skipped: [GUIDE_HELD] }));
    fake.readDocument.mockResolvedValue(view("docs/guide.md"));
    const done = fileops.rename("docs/api.md", "reference.md");
    await vi.waitFor(() => expect(fileops.prompt).not.toBeNull());
    expect(fileops.prompt?.preview.skipped).toEqual([GUIDE_HELD]);
    expect(skipNotes(fileops.prompt?.preview.skipped ?? [])).toEqual([GUIDE_NOTE]);
    fileops.answer("apply");
    await done;
    expect(fake.applyMove).toHaveBeenCalledWith(7, true, []);
  });

  it("holds nothing back when the Visual document has no unsaved edits: it is rewritten on disk and looked at again", async () => {
    await openInVisual(false);
    fake.previewMove.mockResolvedValue(preview(1, REFERENCE));
    fake.applyMove.mockResolvedValue(outcome(REFERENCE, { filesUpdated: 1, linksUpdated: 1 }));
    fake.readDocument.mockResolvedValue(view("docs/guide.md"));
    session.setLinkUpdate("always");
    fake.documentSource.mockClear();
    await fileops.rename("docs/api.md", "reference.md");
    expect(fake.previewMove).toHaveBeenCalledWith(REFERENCE, true, []);
    expect(fake.applyMove).toHaveBeenCalledWith(7, true, []);
    // the editor reads the file again, as it does for any change from outside
    await vi.waitFor(() => expect(fake.documentSource).toHaveBeenCalledWith("docs/guide.md"));
  });

  it("says when the links of a document could not be updated", async () => {
    fake.previewMove.mockResolvedValue(preview(2, RENAME));
    fake.applyMove.mockResolvedValue(
      outcome(RENAME, {
        filesUpdated: 1,
        linksUpdated: 1,
        skipped: [{ path: "docs/api.md", reason: "changed" }],
      }),
    );
    session.setLinkUpdate("always");
    await fileops.rename("docs/guide.md", "manual.md");
    expect(toast.current).toMatchObject({
      tone: "warn",
      text: "Renamed guide.md to manual.md, 1 link updated. 1 document wasn't updated",
    });
    expect(feed.entries[0]).toMatchObject({ notes: ["api.md: changed in the meantime"] });
  });

  it("does one thing at a time", async () => {
    fake.previewMove.mockImplementation(
      () => new Promise<MovePreview>(() => {}), // never answers
    );
    void fileops.rename("docs/guide.md", "a.md");
    await fileops.move(["docs/api.md"], "archive");
    expect(fake.previewMove).toHaveBeenCalledTimes(1);
  });

  it("cut and paste moves into the folder of the row", async () => {
    const moves = [{ from: "README.md", to: "archive/README.md" }];
    fake.previewMove.mockResolvedValue(preview(0, moves));
    fake.applyMove.mockResolvedValue(outcome(moves));
    fileops.cutPicked(["README.md", "docs"]);
    expect([...fileops.cut].sort()).toEqual(["README.md", "docs"]);
    fileops.cutPicked(["README.md"]);
    await fileops.paste("archive");
    expect(fake.previewMove).toHaveBeenCalledWith(moves, true, []);
    expect(fileops.cut.size).toBe(0);
    // pasting into the folder of the item itself, or with nothing cut, does nothing
    fake.previewMove.mockClear();
    await fileops.paste("");
    fileops.cutPicked(["docs/api.md"]);
    await fileops.paste("docs");
    expect(fake.previewMove).not.toHaveBeenCalled();
  });
});

describe("deleting", () => {
  it("asks naming the item, then sends it to the recycle bin", async () => {
    fake.deleteEntries.mockResolvedValue({ deleted: ["docs/api.md"] } satisfies DeleteOutcome);
    await fileops.remove(["docs/api.md"]);
    expect(fake.askYesNo).toHaveBeenCalledWith(
      "Move api.md to the recycle bin?",
      "Move to recycle bin",
      "Cancel",
    );
    expect(fake.deleteEntries).toHaveBeenCalledWith(["docs/api.md"]);
    expect(feed.entries[0]).toMatchObject({ kind: "deleted", summary: "Deleted 1 file" });
  });

  it("names the count for several items, and what is inside a folder goes with it", async () => {
    fake.deleteEntries.mockResolvedValue({ deleted: ["docs", "README.md"] });
    await fileops.remove(["docs", "docs/api.md", "README.md"]);
    expect(fake.askYesNo.mock.calls[0][0]).toBe("Move 2 items to the recycle bin?");
    expect(fake.deleteEntries).toHaveBeenCalledWith(["docs", "README.md"]);
    expect(feed.entries[0]).toMatchObject({ summary: "Deleted 2 items" });
  });

  it("asks differently about a folder", async () => {
    fake.deleteEntries.mockResolvedValue({ deleted: ["docs"] });
    await fileops.remove(["docs"]);
    expect(fake.askYesNo.mock.calls[0][0]).toBe(
      "Move the folder docs and everything in it to the recycle bin?",
    );
  });

  it("does nothing when the answer is no", async () => {
    fake.askYesNo.mockResolvedValue(false);
    await fileops.remove(["docs/api.md"]);
    expect(fake.deleteEntries).not.toHaveBeenCalled();
    expect(feed.entries).toHaveLength(0);
  });

  it("the roots are never deleted, and an empty list is nothing", async () => {
    await fileops.remove([""]);
    await fileops.remove(["@wt/feature"]);
    await fileops.remove([]);
    expect(fake.askYesNo).not.toHaveBeenCalled();
  });

  it("tells the open document it is gone", async () => {
    docs.current = view("docs/api.md");
    fake.deleteEntries.mockResolvedValue({ deleted: ["docs"] });
    await fileops.remove(["docs"]);
    expect(docs.deleted).not.toBeNull();
  });

  it("shows why it could not delete", async () => {
    fake.deleteEntries.mockRejectedValue({ kind: "trash", message: "the bin is full" });
    await fileops.remove(["README.md"]);
    expect(toast.current).toMatchObject({
      tone: "warn",
      text: "Couldn't delete: the bin is full",
    });
  });

  it("looks at the folder again when the bin failed, because it may have taken some of the items", async () => {
    fake.deleteEntries.mockRejectedValue({ kind: "trash", message: "the bin is full" });
    fake.listTree.mockClear();
    await fileops.remove(["README.md", "docs/api.md"]);
    expect(fake.listTree).toHaveBeenCalled();
  });

  it("warns that the open document has changes that are not saved", async () => {
    edit.path = "docs/api.md";
    edit.dirty = true;
    fake.deleteEntries.mockResolvedValue({ deleted: ["docs"] });
    await fileops.remove(["docs"]);
    const question = fake.askYesNo.mock.calls[0][0] as string;
    expect(question).toContain("Move the folder docs and everything in it to the recycle bin?");
    expect(question).toContain(
      "api.md is open with changes that are not saved. They will be lost.",
    );
  });

  it("does not warn when the document that is open has nothing unsaved, or is not deleted", async () => {
    edit.path = "docs/api.md";
    edit.dirty = false;
    fake.deleteEntries.mockResolvedValue({ deleted: ["docs"] });
    await fileops.remove(["docs"]);
    expect(fake.askYesNo.mock.calls[0][0]).not.toContain("not saved");

    edit.dirty = true;
    fake.askYesNo.mockClear();
    await fileops.remove(["README.md"]);
    expect(fake.askYesNo.mock.calls[0][0]).not.toContain("not saved");
  });
});

describe("new folders", () => {
  it("makes the folder under its parent and reports it", async () => {
    fake.newFolder.mockResolvedValue({ opId: 5, path: "docs/ideas" } satisfies CreatedFolder);
    fileops.startNewFolder("docs");
    expect(fileops.newFolder?.folder).toBe("docs");
    expect(vault.expanded.has("docs")).toBe(true);
    expect(await fileops.createFolder("docs", "ideas")).toBeNull();
    expect(fake.newFolder).toHaveBeenCalledWith("docs/ideas");
    expect(fileops.newFolder).toBeNull();
    expect(feed.entries[0]).toMatchObject({ kind: "folder", summary: "Created the folder ideas" });
  });

  it("hands back why it could not be made, for the name field", async () => {
    fake.newFolder.mockRejectedValue({ kind: "exists", message: "docs already exists" });
    expect(await fileops.createFolder("", "docs")).toBe("Something with this name is already here");
    fake.newFolder.mockRejectedValue({ kind: "io", message: "denied" });
    expect(await fileops.createFolder("", "x")).toBe("denied");
  });
});

describe("undo", () => {
  it("puts the last operation back, follows it and reports it", async () => {
    fake.previewMove.mockResolvedValue(preview(0, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME));
    await fileops.rename("docs/guide.md", "manual.md");
    fake.undoFileOp.mockResolvedValue({
      movedBack: [{ from: "docs/manual.md", to: "docs/guide.md" }],
      filesRestored: 0,
      linksRestored: 0,
      foldersRemoved: 0,
      couldNot: [],
    } satisfies UndoOutcome);
    await fileops.undo();
    expect(fake.undoFileOp).toHaveBeenCalledWith(11);
    expect(feed.entries[0]).toMatchObject({
      kind: "undone",
      summary: "Undone: Renamed guide.md to manual.md",
    });
    // there is nothing more to undo
    await fileops.undo();
    expect(toast.current).toMatchObject({ text: "Nothing to undo" });
    expect(fake.undoFileOp).toHaveBeenCalledTimes(1);
  });

  it("the toast's Undo undoes that operation", async () => {
    fake.previewMove.mockResolvedValue(preview(0, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME));
    await fileops.rename("docs/guide.md", "manual.md");
    fake.undoFileOp.mockResolvedValue({
      movedBack: [],
      filesRestored: 0,
      linksRestored: 0,
      foldersRemoved: 0,
      couldNot: [],
    });
    toast.runAction();
    await vi.waitFor(() => expect(fake.undoFileOp).toHaveBeenCalledWith(11));
  });

  it("says what could not be put back", async () => {
    fake.previewMove.mockResolvedValue(preview(0, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME));
    await fileops.rename("docs/guide.md", "manual.md");
    fake.undoFileOp.mockResolvedValue({
      movedBack: [{ from: "docs/manual.md", to: "docs/guide.md" }],
      filesRestored: 0,
      linksRestored: 0,
      foldersRemoved: 0,
      couldNot: [{ path: "README.md", reason: "changed" }],
    });
    await fileops.undo();
    expect(toast.current).toMatchObject({
      tone: "warn",
      text: "Undone: Renamed guide.md to manual.md. 1 file couldn't be put back because it changed",
    });
  });

  it("undoes a new folder", async () => {
    fake.newFolder.mockResolvedValue({ opId: 5, path: "ideas" });
    await fileops.createFolder("", "ideas");
    fake.undoFileOp.mockResolvedValue({
      movedBack: [],
      filesRestored: 0,
      linksRestored: 0,
      foldersRemoved: 1,
      couldNot: [],
    });
    await fileops.undo();
    expect(fake.undoFileOp).toHaveBeenCalledWith(5);
  });

  it("is forgotten when another project is opened", async () => {
    fake.previewMove.mockResolvedValue(preview(0, RENAME));
    fake.applyMove.mockResolvedValue(outcome(RENAME));
    await fileops.rename("docs/guide.md", "manual.md");
    fileops.reset();
    await fileops.undo();
    expect(fake.undoFileOp).not.toHaveBeenCalled();
  });
});

describe("the question about links", () => {
  it("is cancelled when the project is replaced", async () => {
    fake.previewMove.mockResolvedValue(preview(3, RENAME));
    const done = fileops.rename("docs/guide.md", "manual.md");
    await vi.waitFor(() => expect(fileops.prompt).not.toBeNull());
    fileops.reset();
    expect(await done).toBeNull();
    expect(fake.applyMove).not.toHaveBeenCalled();
  });
});
