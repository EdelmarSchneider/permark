import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import { mockHash } from "../../api/mockEdit";
import { confirmLeave } from "./guard";
import { EditingSession, type ActiveEditor, type Hooks } from "./session.svelte";

/** An editor on screen whose text the test types. */
function fakeEditor(initial: string): ActiveEditor & { set(text: string): void } {
  let text = initial;
  return {
    kind: "source",
    text: () => text,
    changes: () => 1,
    topLine: () => 1,
    selection: () => "",
    focus: () => {},
    set: (next) => {
      text = next;
    },
  };
}

function setup() {
  const api = createMockApi();
  const session = new EditingSession(api);
  const notes: string[] = [];
  const opened: string[] = [];
  let saved = 0;
  const hooks: Hooks = {
    note: (text) => notes.push(text),
    fail: (error) => notes.push(`fail ${error.kind}`),
    saved: () => {
      saved += 1;
    },
    open: (path) => opened.push(path),
    readingLine: () => 3,
    showReadingLine: () => {},
    savedText: (count) => `saved ${count}`,
  };
  session.bind(hooks);
  return { api, session, notes, opened, saved: () => saved };
}

async function open(session: EditingSession, path = "docs/guide.md") {
  await session.enter("source", path, true, { kind: "explicit" });
  const content = session.content;
  if (!content) throw new Error("no content");
  const editor = fakeEditor(content.text);
  session.attach(content.seq, editor);
  return { editor, content };
}

/** Lets the change check and the effects run. */
async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(200);
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("editing session", () => {
  it("loads clean, turns dirty on an edit, saves only the changed lines, and is clean again", async () => {
    const { api, session, saved } = setup();
    const { editor, content } = await open(session);
    expect(session.state.s).toBe("clean");
    expect(content.line).toBe(3);
    const lines = content.text.split("\n");
    lines[0] = "# Changed title";
    editor.set(lines.join("\n"));
    session.changed();
    await settle();
    expect(session.state.s).toBe("dirty");
    expect(api.probe.edit.unsaved).toBe("docs/guide.md");
    session.save();
    await settle();
    expect(session.state.s).toBe("clean");
    expect(api.probe.edit.saves).toEqual([
      { path: "docs/guide.md", edits: [{ at: 0, remove: 1, insert: ["# Changed title"] }] },
    ]);
    expect(api.probe.edit.unsaved).toBeNull();
    expect(saved()).toBe(1);
  });

  it("typing and taking it back is no change", async () => {
    const { session } = setup();
    const { editor, content } = await open(session);
    editor.set(content.text + "x");
    session.changed();
    await settle();
    expect(session.state.s).toBe("dirty");
    editor.set(content.text);
    session.changed();
    await settle();
    expect(session.state.s).toBe("clean");
  });

  it("a disk change while dirty is a conflict; keep mine saves over the disk's version", async () => {
    const { api, session } = setup();
    const { editor, content } = await open(session);
    editor.set(content.text + "mine\n");
    session.changed();
    await settle();
    // an agent wrote the file meanwhile
    const theirs = content.text.replace("Guide", "Their guide");
    api.probe.edit.files.set("docs/guide.md", theirs);
    session.onDocumentChanged({
      window: "main",
      path: "docs/guide.md",
      change: "changed",
      version: mockHash(theirs),
    });
    await settle();
    expect(session.state.s).toBe("conflict");
    expect(session.disk?.text).toBe(theirs);
    session.dispatch({ e: "keepMine" });
    await settle();
    expect(session.state.s).toBe("clean");
    expect(api.probe.edit.files.get("docs/guide.md")).toBe(content.text + "mine\n");
  });

  it("a save that meets another version is a conflict and nothing is written", async () => {
    const { api, session } = setup();
    const { editor, content } = await open(session);
    editor.set(content.text + "mine\n");
    session.changed();
    await settle();
    api.probe.edit.files.set("docs/guide.md", "someone else\n");
    session.save();
    await settle();
    expect(session.state.s).toBe("conflict");
    expect(api.probe.edit.files.get("docs/guide.md")).toBe("someone else\n");
    expect(api.probe.edit.saves).toEqual([]);
  });

  it("save as new opens the person's copy", async () => {
    const { session, opened, api } = setup();
    const { editor, content } = await open(session);
    editor.set(content.text + "mine\n");
    session.changed();
    await settle();
    api.probe.edit.files.set("docs/guide.md", "theirs\n");
    session.onDocumentChanged({
      window: "main",
      path: "docs/guide.md",
      change: "changed",
      version: mockHash("theirs\n"),
    });
    await settle();
    session.dispatch({ e: "saveAsNew" });
    await settle();
    expect(opened).toEqual(["docs/guide-mine.md"]);
    expect(api.probe.edit.files.get("docs/guide-mine.md")).toBe(content.text + "mine\n");
    expect(session.state.s).toBe("closed");
  });

  it("asks before leaving unsaved edits: keep editing stays, discard leaves", async () => {
    const { session, api } = setup();
    const { editor, content } = await open(session);
    editor.set(content.text + "x");
    session.changed();
    await settle();
    const stay = confirmLeave({ kind: "document", path: "README.md" });
    expect(session.state.s).toBe("unsavedPrompt");
    session.answer("keepEditing");
    expect(await stay).toBe(false);
    expect(session.state.s).toBe("dirty");
    const go = confirmLeave({ kind: "document", path: "README.md" });
    session.answer("discard");
    expect(await go).toBe(true);
    await settle();
    expect(session.state.s).toBe("closed");
    expect(api.probe.edit.saves).toEqual([]);
    expect(api.probe.edit.unsaved).toBeNull();
    // nothing held: leaving is free
    expect(await confirmLeave({ kind: "window" })).toBe(true);
  });

  it("save then leave: the prompt's Save writes first", async () => {
    const { session, api } = setup();
    const { editor, content } = await open(session);
    editor.set(content.text + "y");
    session.changed();
    await settle();
    const go = confirmLeave({ kind: "window" });
    session.answer("save");
    await settle();
    expect(await go).toBe(true);
    expect(api.probe.edit.saves.length).toBe(1);
  });

  it("a clean document leaves without asking; the same document is not a leave", async () => {
    const { session } = setup();
    await open(session);
    expect(await confirmLeave({ kind: "document", path: "docs/guide.md" })).toBe(true);
    expect(session.state.s).toBe("clean");
    expect(await confirmLeave({ kind: "document", path: "README.md" })).toBe(true);
    expect(session.state.s).toBe("closed");
  });

  it("switches Edit <-> Source keeping the text, and to View after asking", async () => {
    const { session } = setup();
    const { editor, content } = await open(session);
    editor.set(content.text + "kept\n");
    session.changed();
    await settle();
    await session.switchTo("edit");
    expect(session.mode).toBe("edit");
    expect(session.content?.text).toBe(content.text + "kept\n");
    expect(session.content?.segments?.length).toBeGreaterThan(0);
    expect(session.state.s).toBe("dirty");
  });
});
