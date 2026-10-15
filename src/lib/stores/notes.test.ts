// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the store asks it, when, and what it keeps meanwhile.
const fake = vi.hoisted(() => ({
  addNote: vi.fn(),
  listNotes: vi.fn(),
  setNoteDone: vi.fn(),
  deleteNote: vi.fn(),
}));
vi.mock("../api", () => ({ api: { ...fake, on: async () => () => {} } }));

import { bridge } from "../bridge";
import type { NoteTarget } from "../dom/noteDom";
import type { DocumentView, FsBatch, Note, VaultInfo } from "../types";
import { docs } from "./docs.svelte";
import { NotesStore, REFRESH_DELAY_MS, REMEMBER_MS } from "./notes.svelte";
import { toast } from "./toast.svelte";
import { vault } from "./vault.svelte";

let counter = 0;
const note = (patch: Partial<Note> = {}): Note => ({
  id: `n${++counter}`,
  path: "docs/plan.md",
  text: "A note",
  quote: "a passage",
  headingPath: ["Plan"],
  line: 3,
  endLine: 3,
  createdMs: 1000 + counter,
  state: "anchored",
  done: false,
  ...patch,
});

const target = (patch: Partial<NoteTarget> = {}): NoteTarget => ({
  quote: "ship it on Friday",
  startLine: 7,
  endLine: 7,
  headingPath: ["Plan", "Rollout"],
  rect: new DOMRect(10, 20, 100, 16),
  range: document.createRange(),
  ...patch,
});

const batch = (...paths: string[]): FsBatch => ({
  changes: paths.map((path) => ({ kind: "modified" as const, path, atMs: 1 })),
  bulk: null,
  rescan: false,
});

let clockMs = 0;
let store: NotesStore;
let selection: NoteTarget | null;

function openDocument(path: string | null, kind: DocumentView["kind"] = "markdown"): void {
  docs.current = path === null ? null : ({ path, kind } as DocumentView);
}

beforeEach(() => {
  vi.useFakeTimers();
  for (const fn of Object.values(fake)) fn.mockReset();
  fake.listNotes.mockResolvedValue([]);
  toast.clear();
  clockMs = 1_000_000;
  store = new NotesStore(() => clockMs);
  selection = target();
  bridge.noteTarget = () => selection;
  openDocument("docs/plan.md");
  docs.editing = false;
  vault.info = { mode: "ide" } as VaultInfo;
});

afterEach(() => {
  bridge.noteTarget = undefined;
  openDocument(null);
  vault.info = null;
  vi.useRealTimers();
});

describe("the list", () => {
  it("is what the backend says, asked for when the project opens", async () => {
    const list = [note({ line: 2 }), note({ done: true })];
    fake.listNotes.mockResolvedValue(list);
    await store.refresh();
    expect(store.all).toEqual(list);
    expect(store.count).toBe(1);
    expect(store.groups.map((g) => g.path)).toEqual(["docs/plan.md"]);
    expect(store.done).toHaveLength(1);
  });

  it("is not asked for in a single file, which has no project", async () => {
    vault.info = { mode: "simple" } as VaultInfo;
    await store.refresh();
    expect(fake.listNotes).not.toHaveBeenCalled();
  });

  it("is not overwritten by an answer that a newer question has overtaken", async () => {
    let answer: (notes: Note[]) => void = () => {};
    fake.listNotes.mockReturnValueOnce(new Promise<Note[]>((resolve) => (answer = resolve)));
    const old = store.refresh();
    fake.listNotes.mockResolvedValueOnce([note({ text: "new" })]);
    await store.refresh();
    answer([note({ text: "old" })]);
    await old;
    expect(store.all.map((n) => n.text)).toEqual(["new"]);
  });

  it("is forgotten when the project changes, and an answer on its way is dropped", async () => {
    let answer: (notes: Note[]) => void = () => {};
    fake.listNotes.mockReturnValueOnce(new Promise<Note[]>((resolve) => (answer = resolve)));
    const pending = store.refresh();
    store.reset();
    answer([note()]);
    await pending;
    expect(store.all).toEqual([]);
  });

  it("keeps what it had when the question fails", async () => {
    fake.listNotes.mockResolvedValue([note()]);
    await store.refresh();
    fake.listNotes.mockRejectedValue(new Error("no"));
    await store.refresh();
    expect(store.all).toHaveLength(1);
  });
});

describe("keeping up with the documents", () => {
  beforeEach(async () => {
    fake.listNotes.mockResolvedValue([note({ path: "docs/plan.md" })]);
    await store.refresh();
    fake.listNotes.mockClear();
  });

  it("asks again, once, a moment after a document with notes is drawn again", async () => {
    store.documentChanged();
    store.documentChanged();
    expect(fake.listNotes).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS);
    expect(fake.listNotes).toHaveBeenCalledTimes(1);
  });

  it("asks again when files with notes change on disk, and not for other files", async () => {
    store.filesChanged(batch("docs/other.md"));
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS * 2);
    expect(fake.listNotes).not.toHaveBeenCalled();
    store.filesChanged(batch("docs/plan.md"));
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS);
    expect(fake.listNotes).toHaveBeenCalledTimes(1);
  });

  it("asks again for a rescan or a bulk change, which could have touched anything", async () => {
    store.filesChanged({ changes: [], bulk: null, rescan: true });
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS);
    expect(fake.listNotes).toHaveBeenCalledTimes(1);
    store.filesChanged({
      changes: [],
      bulk: { count: 900, reason: "gitOperation", paths: [] },
      rescan: false,
    });
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS);
    expect(fake.listNotes).toHaveBeenCalledTimes(2);
  });

  it("asks at once when the backend says the notes changed", async () => {
    store.onChanged();
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.listNotes).toHaveBeenCalledTimes(1);
  });

  it("does not ask while there are no notes: no document can have gained one", async () => {
    store.reset();
    store.documentChanged();
    store.filesChanged(batch("docs/plan.md"));
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS * 2);
    expect(fake.listNotes).not.toHaveBeenCalled();
  });
});

describe("writing a note", () => {
  it("opens the field on the selection, with the document it is in", () => {
    expect(store.begin()).toBe(true);
    expect(store.draft?.path).toBe("docs/plan.md");
    expect(store.draft?.target.quote).toBe("ship it on Friday");
  });

  it("says to select something first when nothing is selected", () => {
    selection = null;
    expect(store.begin()).toBe(false);
    expect(store.draft).toBeNull();
    expect(toast.current?.type === "message" && toast.current.text).toBe(
      "Select a passage in the document first, then add the note.",
    );
  });

  it("does nothing outside a Markdown document of a project", () => {
    openDocument("data.json", "json" as DocumentView["kind"]);
    expect(store.begin()).toBe(false);
    openDocument("docs/plan.md");
    docs.editing = true;
    expect(store.begin()).toBe(false);
    docs.editing = false;
    vault.info = { mode: "simple" } as VaultInfo;
    expect(store.begin()).toBe(false);
    openDocument(null);
    vault.info = { mode: "ide" } as VaultInfo;
    expect(store.begin()).toBe(false);
    expect(toast.current).toBeNull();
  });

  it("uses the selection seen a moment ago when something took it away", () => {
    store.remember();
    selection = null;
    expect(store.begin()).toBe(true);
    expect(store.draft?.target.quote).toBe("ship it on Friday");
  });

  it("forgets that selection after a minute, and after another document was opened", () => {
    store.remember();
    selection = null;
    clockMs += REMEMBER_MS;
    expect(store.begin()).toBe(false);

    store.remember();
    openDocument("docs/other.md");
    expect(store.begin()).toBe(false);
  });

  it("prefers the selection on screen to the one remembered", () => {
    store.remember();
    selection = target({ quote: "another passage" });
    store.begin();
    expect(store.draft?.target.quote).toBe("another passage");
  });

  it("saves the note with the passage as the view knew it, then closes the field and asks for the list", async () => {
    store.begin();
    fake.addNote.mockResolvedValue(note());
    fake.listNotes.mockResolvedValue([note({ text: "Too vague." })]);
    expect(await store.save("  Too vague.  ")).toBe(true);
    expect(fake.addNote).toHaveBeenCalledWith({
      path: "docs/plan.md",
      quote: "ship it on Friday",
      text: "Too vague.",
      headingPath: ["Plan", "Rollout"],
      line: 7,
      endLine: 7,
    });
    expect(store.draft).toBeNull();
    expect(store.all.map((n) => n.text)).toEqual(["Too vague."]);
    expect(store.saving).toBe(false);
  });

  it("does not save an empty note", async () => {
    store.begin();
    expect(await store.save("   ")).toBe(false);
    expect(fake.addNote).not.toHaveBeenCalled();
    expect(store.draft).not.toBeNull();
  });

  it("keeps the field open, with a message, when the note cannot be saved", async () => {
    store.begin();
    fake.addNote.mockRejectedValue({ kind: "emptyNote", message: "x" });
    expect(await store.save("Words")).toBe(false);
    expect(store.draft).not.toBeNull();
    expect(store.saving).toBe(false);
    expect(toast.current?.type === "message" && toast.current.text).toContain(
      "Couldn't save the note",
    );
  });

  it("saves once while a save is on its way", async () => {
    store.begin();
    let finish: (n: Note) => void = () => {};
    fake.addNote.mockReturnValue(new Promise<Note>((resolve) => (finish = resolve)));
    const first = store.save("Words");
    expect(await store.save("Words")).toBe(false);
    finish(note());
    await first;
    expect(fake.addNote).toHaveBeenCalledTimes(1);
  });

  it("closes without a note", () => {
    store.begin();
    store.cancel();
    expect(store.draft).toBeNull();
    store.cancel();
  });
});

describe("the list's own actions", () => {
  it("checks a note off and keeps what the backend answered", async () => {
    const open = note();
    fake.listNotes.mockResolvedValue([open]);
    await store.refresh();
    fake.setNoteDone.mockResolvedValue({ ...open, done: true, doneMs: 5, doneBy: "person" });
    await store.setDone(open.id, true);
    expect(fake.setNoteDone).toHaveBeenCalledWith(open.id, true);
    expect(store.count).toBe(0);
    expect(store.done.map((n) => n.id)).toEqual([open.id]);
  });

  it("asks the backend again when it could not change a note", async () => {
    const open = note();
    fake.listNotes.mockResolvedValue([open]);
    await store.refresh();
    fake.setNoteDone.mockRejectedValue({ kind: "noteNotFound", message: "gone" });
    fake.listNotes.mockClear();
    await store.setDone(open.id, true);
    expect(fake.listNotes).toHaveBeenCalledTimes(1);
    expect(toast.current?.type).toBe("message");
  });

  it("deletes a note", async () => {
    const [one, two] = [note(), note()];
    fake.listNotes.mockResolvedValue([one, two]);
    await store.refresh();
    fake.deleteNote.mockResolvedValue(undefined);
    await store.remove(one.id);
    expect(fake.deleteNote).toHaveBeenCalledWith(one.id);
    expect(store.all.map((n) => n.id)).toEqual([two.id]);
  });

  it("asks the panel to show a note, again on every request", () => {
    store.show("n1");
    const first = store.focus;
    store.show("n1");
    expect(store.focus?.id).toBe("n1");
    expect(store.focus?.n).toBeGreaterThan(first?.n ?? 0);
  });
});
