import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend and the system are replaced: these tests are about what the stores say the window shows, and the title that makes.
vi.mock("../api", () => ({
  api: { saveSession: vi.fn(async () => {}), on: async () => () => {} },
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

import type { DocumentView, NoteEntry, VaultInfo } from "../types";
import { windowTitle } from "../windowTitle";
import { app } from "./app.svelte";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { notes } from "./notes.svelte";
import { vault } from "./vault.svelte";
import { currentTitleState } from "./windowTitleState";

const title = (): string => windowTitle(currentTitleState());

const info = (mode: VaultInfo["mode"], name: string): VaultInfo => ({
  root: `C:/work/${name}`,
  name,
  kind: "project",
  mode,
  initialFile: null,
  worktrees: [],
  liveUpdates: true,
  treeDefault: null,
});

const view = (path: string): DocumentView => ({ path }) as DocumentView;

const entry = (path: string, extra: Partial<NoteEntry> = {}): NoteEntry => ({
  kind: "note",
  path,
  title: "",
  snippet: "",
  modifiedMs: 0,
  sizeBytes: 0,
  pinned: false,
  copied: false,
  view: null,
  folder: null,
  missing: false,
  unsaved: false,
  shared: false,
  ...extra,
});

describe("the title of the window, from the stores", () => {
  beforeEach(() => {
    app.screen = "welcome";
    vault.reset();
    docs.reset();
    edit.dirty = false;
    edit.autosave = false;
    edit.path = null;
    notes.current = null;
    notes.entries = [];
  });

  it("is the name of the program on the welcome screen", () => {
    expect(title()).toBe("TrailMark");
  });

  describe("in a project", () => {
    beforeEach(() => {
      app.screen = "project";
      vault.info = info("ide", "trailmark-docs");
    });

    it("names the project while no document is open", () => {
      expect(title()).toBe("trailmark-docs - TrailMark");
    });

    it("follows the document, by its file name", () => {
      docs.current = view("docs/design/v0.5.md");
      expect(title()).toBe("v0.5.md - trailmark-docs - TrailMark");
      docs.current = view("@wt/feature-ui/README.md");
      expect(title()).toBe("README.md - trailmark-docs - TrailMark");
    });

    it("follows a rename of the open file", () => {
      docs.current = view("plan.md");
      expect(title()).toBe("plan.md - trailmark-docs - TrailMark");
      docs.current = view("docs/roadmap.md");
      expect(title()).toBe("roadmap.md - trailmark-docs - TrailMark");
    });

    it("marks unsaved edits, and drops the mark once they are saved", () => {
      docs.current = view("plan.md");
      edit.dirty = true;
      expect(title()).toBe("* plan.md - trailmark-docs - TrailMark");
      edit.dirty = false;
      expect(title()).toBe("plan.md - trailmark-docs - TrailMark");
    });

    it("goes back to the welcome title when the project is closed", () => {
      docs.current = view("plan.md");
      app.screen = "welcome";
      expect(title()).toBe("TrailMark");
    });

    it("does not name the folder of a single file", () => {
      vault.info = info("simple", "Downloads");
      docs.current = view("proposal.md");
      expect(title()).toBe("proposal.md - TrailMark");
      edit.dirty = true;
      expect(title()).toBe("* proposal.md - TrailMark");
    });
  });

  describe("in the Notes window", () => {
    beforeEach(() => {
      app.screen = "notes";
      edit.autosave = true;
    });

    it("says Notes while no note is open", () => {
      expect(title()).toBe("Notes - TrailMark");
    });

    it("names the note by its title", () => {
      notes.current = "shopping.md";
      notes.entries = [entry("shopping.md", { title: "Shopping list" })];
      expect(title()).toBe("Shopping list - Notes - TrailMark");
    });

    it("names a note with no text as the header does", () => {
      notes.current = "new.md";
      notes.entries = [entry("new.md")];
      expect(title()).toBe("Untitled note - Notes - TrailMark");
    });

    it("has no unsaved mark for a note, which is saved as it is typed", () => {
      notes.current = "shopping.md";
      notes.entries = [entry("shopping.md", { title: "Shopping list" })];
      edit.path = "shopping.md";
      edit.dirty = true;
      expect(title()).toBe("Shopping list - Notes - TrailMark");
    });

    it("names a loose file by its file name", () => {
      notes.current = "@file/3/todo.txt";
      expect(title()).toBe("todo.txt - Notes - TrailMark");
      notes.entries = [entry("@file/3/todo.txt", { kind: "file", title: "todo.txt" })];
      expect(title()).toBe("todo.txt - Notes - TrailMark");
    });

    it("marks a loose file with unsaved edits, typed now or kept from before", () => {
      notes.current = "@file/3/todo.txt";
      notes.entries = [entry("@file/3/todo.txt", { kind: "file", title: "todo.txt" })];
      edit.path = "@file/3/todo.txt";
      edit.dirty = true;
      expect(title()).toBe("* todo.txt - Notes - TrailMark");
      edit.dirty = false;
      expect(title()).toBe("todo.txt - Notes - TrailMark");
      notes.entries = [
        entry("@file/3/todo.txt", { kind: "file", title: "todo.txt", unsaved: true }),
      ];
      expect(title()).toBe("* todo.txt - Notes - TrailMark");
    });
  });
});
