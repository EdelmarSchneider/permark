import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi, SourceApi, VisualApi } from "../bridge";
import type { DocMode } from "../editMode";
import type { DocumentView, ParsedBlocks, SourceDoc } from "../types";

// The header's Read | Visual | Source switch and its shortcuts (Ctrl+E, E, Ctrl+Alt+E), seen from the store: which mode each step
// leads to, what the last editor used is, when leaving asks first, and which line is kept. The backend and the native dialogs
// are replaced; the editors are fakes that share one text, the way the real ones are mounted one after the other.
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
import { vault } from "./vault.svelte";

const PATH = "docs/a.md";
const ORIGINAL = "# A\n\nfirst\n\nsecond\n";

const view = (kind: DocumentView["kind"] = "markdown", path = PATH): DocumentView => ({
  path,
  kind,
  html: "",
  blocks: [],
  title: null,
  frontmatter: null,
  modifiedMs: 1,
  sizeBytes: 1,
  encodingNote: null,
});
const source = (text: string): SourceDoc => ({
  path: PATH,
  text,
  hash: "h1",
  modifiedMs: 100,
  ...mockParseBlocks(text),
});

/** What the person sees and types, shared by whichever editor is mounted. */
const screen = { text: ORIGINAL, caretLine: 0, topLine: 0 };

function fakeSource(): SourceApi {
  return {
    ...noOutline,
    mode: "source",
    getText: () => screen.text,
    setText: (text) => void (screen.text = text),
    caretLine: () => screen.caretLine,
    topLine: () => screen.topLine,
    focus: vi.fn(),
    insertImage: vi.fn(),
    applyChanges: vi.fn(),
  };
}

function fakeVisual(): VisualApi {
  return {
    ...noOutline,
    mode: "visual",
    text: () => screen.text,
    status: () => ({ dirty: screen.text !== ORIGINAL, changed: screen.text === ORIGINAL ? 0 : 1 }),
    caretLine: () => screen.caretLine,
    topLine: () => screen.topLine,
    caretBlock: () => 0,
    lineOfBlock: (block) => (block === null ? screen.caretLine : block * 2),
    load: vi.fn(),
    rebase: vi.fn(),
    focus: vi.fn(),
    insertImage: vi.fn(),
  };
}

/** What the components do when the store changes mode: the editor of that mode is mounted, and none is left once editing ends. */
function sync(): void {
  if (!edit.active) bridge.editor = undefined;
  else if (bridge.editor?.mode !== edit.mode) {
    const editor: EditorApi = edit.mode === "visual" ? fakeVisual() : fakeSource();
    bridge.editor = editor;
  }
}

/** Clicks a segment of the header switch (`goTo`). */
async function go(target: DocMode): Promise<void> {
  await edit.goTo(target);
  sync();
}

/** Goes back to Read from an editor with nothing unsaved. */
async function toRead(): Promise<void> {
  await go("read");
  await vi.waitFor(() => expect(edit.active).toBe(false));
  sync();
}

/** Types into the editor on screen. */
function type(text: string): void {
  screen.text = text;
  if (edit.mode === "source") edit.noteChange(text.length);
  else edit.noteVisual();
}

/** The shortcuts, as the keyboard handler runs them. */
async function press(action: "edit" | "toggleEdit" | "toggleEditMode"): Promise<void> {
  app.run(action);
  await vi.waitFor(() => expect(edit.loading || edit.switching).toBe(false));
  sync();
}

beforeEach(() => {
  session.editMode = "visual";
  edit.reset();
  docs.reset();
  docs.guard = (resume) => edit.holdNavigation(resume);
  bridge.editor = undefined;
  bridge.topLine = undefined;
  screen.text = ORIGINAL;
  screen.caretLine = 0;
  screen.topLine = 0;
  for (const mock of Object.values(fake)) mock.mockReset();
  fake.parseBlocks.mockImplementation(async (text: string): Promise<ParsedBlocks> =>
    mockParseBlocks(text),
  );
  fake.documentSource.mockResolvedValue(source(ORIGINAL));
  fake.saveDocument.mockResolvedValue({ result: "saved", hash: "h2", modifiedMs: 200 });
  fake.readDocument.mockResolvedValue(view());
  docs.current = view();
  vault.info = { mode: "project", root: "/p", name: "p", worktrees: [] } as never;
  app.screen = "project";
});

describe("from Read", () => {
  it("Visual and Source each open their editor", async () => {
    await go("visual");
    expect(edit.active).toBe(true);
    expect(edit.mode).toBe("visual");
    await toRead();
    await go("source");
    expect(edit.mode).toBe("source");
    expect(edit.initial?.buffer).toBe(ORIGINAL);
  });

  it("Read is already there: nothing happens", async () => {
    await edit.goTo("read");
    expect(edit.active).toBe(false);
    expect(fake.documentSource).not.toHaveBeenCalled();
  });

  it("opens either editor on the block at the top of the viewport, and keeps that block at the top", async () => {
    bridge.topLine = () => 3;
    await go("source");
    expect(edit.initial).toMatchObject({ line: 2, top: 2 });
    await toRead();
    await go("visual");
    expect(edit.visualStart).toMatchObject({ block: 1, reveal: "start" });
  });

  it("a document that cannot be edited stays in Read, and says why", async () => {
    docs.current = view("binary");
    const warn = vi.spyOn(toast, "message");
    expect(edit.blocked).toMatch(/Binary files can't be edited/);
    await go("visual");
    await go("source");
    expect(edit.active).toBe(false);
    expect(fake.documentSource).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalled();
  });
});

describe("between the editors", () => {
  it("keeps unsaved work both ways, and never asks", async () => {
    await go("visual");
    type("# A\n\nfirst, changed\n\nsecond\n");
    expect(edit.dirty).toBe(true);
    await go("source");
    expect(edit.mode).toBe("source");
    expect(edit.prompt).toBeNull();
    expect(edit.dirty).toBe(true);
    expect(edit.initial?.buffer).toBe("# A\n\nfirst, changed\n\nsecond\n");
    await go("visual");
    expect(edit.mode).toBe("visual");
    expect(edit.prompt).toBeNull();
    expect(edit.dirty).toBe(true);
    expect(edit.visualStart?.map.blocks.map((b) => b.text)).toContain("first, changed");
  });

  it("clicking the mode already shown does nothing", async () => {
    await go("source");
    const initial = edit.initial;
    await edit.goTo("source");
    expect(edit.mode).toBe("source");
    expect(edit.initial).toBe(initial);
  });

  it("keeps the block at the top of the view, whichever block the caret is in", async () => {
    await go("visual");
    screen.topLine = 4;
    screen.caretLine = 2;
    await go("source");
    expect(edit.initial).toMatchObject({ line: 2, top: 4 });
    screen.topLine = 2;
    screen.caretLine = 4;
    await go("visual");
    // line 4 is the block "second" (the caret's), line 2 the block "first" (at the top)
    expect(edit.visualStart).toMatchObject({ block: 2, top: 1 });
  });
});

describe("back to Read", () => {
  it("from a clean editor leaves at once, with the block at the top of the view at the top", async () => {
    await go("source");
    screen.topLine = 4;
    await toRead();
    expect(edit.prompt).toBeNull();
    await vi.waitFor(() => expect(docs.jump?.locate).toMatchObject({ line: 5, align: "top" }));
  });

  it("from an editor with unsaved work asks first (Save, Discard, Keep editing)", async () => {
    await go("visual");
    type("# A\n\nfirst, changed\n\nsecond\n");
    await edit.goTo("read");
    expect(edit.prompt?.reason).toBe("exit");
    expect(edit.active).toBe(true);
    edit.keepEditing();
    expect(edit.prompt).toBeNull();
    expect(edit.mode).toBe("visual");
    expect(edit.dirty).toBe(true);
  });

  it("Discard leaves without saving", async () => {
    await go("source");
    type(`${ORIGINAL}more\n`);
    await edit.goTo("read");
    edit.promptDiscard();
    await vi.waitFor(() => expect(edit.active).toBe(false));
    expect(fake.saveDocument).not.toHaveBeenCalled();
  });

  it("Save writes the unsaved work, then leaves", async () => {
    await go("source");
    type(`${ORIGINAL}more\n`);
    await edit.goTo("read");
    await edit.promptSave();
    await vi.waitFor(() => expect(edit.active).toBe(false));
    expect(fake.saveDocument).toHaveBeenCalledTimes(1);
    expect(fake.saveDocument.mock.calls[0][1]).toBe(`${ORIGINAL}more\n`);
  });

  it("Save that fails stays in the editor with the work", async () => {
    await go("visual");
    type("# A\n\nfirst, changed\n\nsecond\n");
    fake.saveDocument.mockRejectedValue(new Error("disk full"));
    await edit.goTo("read");
    await edit.promptSave();
    expect(edit.active).toBe(true);
    expect(edit.saveError).toBe("disk full");
  });

  it("work that was saved leaves without asking", async () => {
    await go("source");
    type(`${ORIGINAL}more\n`);
    expect(await edit.save()).toBe(true);
    await edit.goTo("read");
    expect(edit.prompt).toBeNull();
    await vi.waitFor(() => expect(edit.active).toBe(false));
  });
});

describe("the editor used last", () => {
  it("is the setting until another editor is used", () => {
    expect(edit.lastEditMode).toBe("visual");
    session.editMode = "source";
    expect(edit.lastEditMode).toBe("source");
  });

  it("is remembered when an editor is entered, switched to, and left", async () => {
    await go("source");
    expect(edit.lastEditMode).toBe("source");
    await go("visual");
    expect(edit.lastEditMode).toBe("visual");
    await go("source");
    await toRead();
    expect(edit.lastEditMode).toBe("source");
  });

  it("is what Ctrl+E and E open from Read", async () => {
    await go("source");
    await toRead();
    await press("toggleEdit");
    expect(edit.mode).toBe("source");
    await press("toggleEdit");
    expect(edit.active).toBe(false);
    await press("edit");
    expect(edit.active).toBe(true);
    expect(edit.mode).toBe("source");
  });

  it("gives way to a change of the setting 'Open files for editing in'", async () => {
    await go("source");
    await toRead();
    expect(edit.lastEditMode).toBe("source");
    session.setEditMode("visual");
    expect(edit.lastEditMode).toBe("visual");
    await press("toggleEdit");
    expect(edit.mode).toBe("visual");
  });

  it("is forgotten when another project is opened", async () => {
    await go("source");
    edit.reset();
    expect(edit.lastEditMode).toBe("visual");
  });
});

describe("Ctrl+E", () => {
  it("toggles Read and the last editor used", async () => {
    await press("toggleEdit");
    expect(edit.mode).toBe("visual");
    await press("toggleEdit");
    expect(edit.active).toBe(false);
    await press("toggleEdit");
    expect(edit.mode).toBe("visual");
  });

  it("asks before leaving an editor with unsaved work", async () => {
    await go("source");
    type(`${ORIGINAL}more\n`);
    await press("toggleEdit");
    expect(edit.prompt?.reason).toBe("exit");
    expect(edit.active).toBe(true);
  });

  it("opens nothing when no document is open or it cannot be edited", async () => {
    docs.current = view("tooLarge");
    await press("toggleEdit");
    expect(edit.active).toBe(false);
    docs.current = null;
    await press("toggleEdit");
    expect(edit.active).toBe(false);
  });
});

describe("Ctrl+Alt+E", () => {
  it("toggles Visual and Source while editing, keeping unsaved work", async () => {
    await go("visual");
    type("# A\n\nfirst, changed\n\nsecond\n");
    await press("toggleEditMode");
    expect(edit.mode).toBe("source");
    expect(edit.dirty).toBe(true);
    await press("toggleEditMode");
    expect(edit.mode).toBe("visual");
    expect(edit.dirty).toBe(true);
    expect(edit.prompt).toBeNull();
  });

  it("from Read enters the editor that was not used last", async () => {
    await press("toggleEditMode");
    // nothing used yet: the setting is Visual, so the other one is Source
    expect(edit.mode).toBe("source");
    await press("toggleEdit");
    expect(edit.active).toBe(false);
    // Source was used last: the other one is Visual
    await press("toggleEditMode");
    expect(edit.mode).toBe("visual");
    await press("toggleEdit");
    expect(edit.active).toBe(false);
    await press("toggleEditMode");
    expect(edit.mode).toBe("source");
  });

  it("from Read opens nothing when the document cannot be edited", async () => {
    docs.current = view("binary");
    await press("toggleEditMode");
    expect(edit.active).toBe(false);
  });
});

describe("E", () => {
  it("enters the last editor used while reading, and does nothing while editing", async () => {
    session.editMode = "source";
    await press("edit");
    expect(edit.mode).toBe("source");
    await press("edit");
    expect(edit.mode).toBe("source");
    expect(edit.active).toBe(true);
  });
});
