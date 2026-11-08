// The mode the person chose (View | Edit | Source) is the mode the next document opens in. The header
// switch and Ctrl+E go through `chooseMode`; a file switch calls `openRemembered`.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentView } from "../../api";
import type { SavePolicy } from "./docMachine";

// the window's editing session talks to the mock backend (one per test file)
vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

// the document this window shows: the test moves it
const shown = vi.hoisted(() => {
  const state = { path: null as string | null, view: null as DocumentView | null };
  return {
    state,
    documents: {
      get path(): string | null {
        return state.path;
      },
      get view(): DocumentView | null {
        return state.view;
      },
      get savePolicy(): SavePolicy {
        return { kind: "explicit" };
      },
      show: (path: string) => {
        state.path = path;
      },
    },
  };
});
vi.mock("../doc/document.svelte", () => ({ documents: shown.documents }));

const { editing, EditingSession } = await import("./session.svelte");
const { api } = await import("../../api");
const { chooseMode, editBlockedReason, openRemembered } = await import("./choose");
const { mockView } = await import("../../api/mockDocs");

/** The view of a markdown file the mock can open (`readOnly`: the page says it cannot be edited). */
function viewFor(path: string, readOnly = false): DocumentView {
  return {
    path,
    name: path.split("/").at(-1) ?? path,
    format: "markdown",
    role: null,
    body: { kind: "markdown", blocks: [], outline: [], frontMatter: null },
    hash: "0000000000000000",
    size: 1024,
    mtimeMs: Date.now(),
    editable: readOnly ? { editable: "readOnly", reason: "media" } : { editable: "surgical" },
    notes: [],
  };
}

/** The window shows `path`. */
function show(path: string, readOnly = false): void {
  shown.state.path = path;
  shown.state.view = viewFor(path, readOnly);
}

/** Lets the load and the effects run. */
async function flush(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

/** The file switch the document area does (`DocumentArea.svelte`: release, then show the new path). */
function switchTo(path: string, readOnly = false): void {
  editing.release();
  show(path, readOnly);
  openRemembered();
}

/** The view the mock backend gives for `path` (Markdown, a config file's sheet, plain text). */
function mockViewOf(path: string): DocumentView {
  const view = mockView(path, [], true);
  if (view === null) throw new Error(`the mock has no ${path}`);
  return view;
}

/** The window shows `path` with the view the mock backend gives for it. */
function showMock(path: string): void {
  shown.state.path = path;
  shown.state.view = mockViewOf(path);
}

/**
 * A file switch as it really comes: the path moves first while the page still holds the previous file's view (the
 * new one loads), and the document area asks again once the new view lands.
 */
async function switchWhileLoading(path: string): Promise<void> {
  editing.release();
  shown.state.path = path;
  openRemembered();
  await flush();
  shown.state.view = mockViewOf(path);
  openRemembered();
  await flush();
}

beforeEach(() => {
  vi.useFakeTimers();
  editing.release();
  // a fresh window: no mode chosen yet
  editing.chosenMode = "view";
  show("docs/guide.md");
});
afterEach(() => {
  vi.useRealTimers();
});

describe("the chosen mode", () => {
  it("opens the first document in Edit when the person chooses Edit", async () => {
    chooseMode("edit");
    await flush();
    expect(editing.mode).toBe("edit");
    expect(editing.path).toBe("docs/guide.md");
  });

  it("opens the next file in the mode the person chose, not back in View", async () => {
    chooseMode("edit");
    await flush();
    expect(editing.mode).toBe("edit");
    switchTo("docs/handbook.md");
    await flush();
    expect(editing.mode).toBe("edit");
    expect(editing.path).toBe("docs/handbook.md");
  });

  it("keeps Source when Source was the mode chosen", async () => {
    chooseMode("source");
    await flush();
    switchTo("docs/handbook.md");
    await flush();
    expect(editing.mode).toBe("source");
    expect(editing.path).toBe("docs/handbook.md");
  });

  it("opens the next file in View after the person went back to View", async () => {
    chooseMode("edit");
    await flush();
    chooseMode("view");
    await flush();
    switchTo("docs/handbook.md");
    await flush();
    expect(editing.mode).toBe("view");
    expect(editing.path).toBeNull();
  });

  it("leaves a document that cannot be edited in View", async () => {
    chooseMode("edit");
    await flush();
    switchTo("img/hero.png", true);
    await flush();
    expect(editing.mode).toBe("view");
    expect(editing.path).toBeNull();
  });

  it("a fresh window starts in View, so nothing is opened", async () => {
    const fresh = new EditingSession(api);
    expect(fresh.mode).toBe("view");
    expect(fresh.chosenMode).toBe("view");
    openRemembered();
    await flush();
    expect(editing.mode).toBe("view");
    expect(editing.path).toBeNull();
  });

  it("does not open the document the editor is already on again", async () => {
    chooseMode("edit");
    await flush();
    const seq = editing.content?.seq ?? 0;
    openRemembered();
    await flush();
    expect(editing.mode).toBe("edit");
    expect(editing.content?.seq).toBe(seq);
  });
});

describe("the editor follows the file shown, not the page still on screen while it loads", () => {
  it("Edit on Markdown, then a config file: the sheet opens on it", async () => {
    chooseMode("edit");
    await flush();
    expect(editing.kind).toBe("markdown");
    await switchWhileLoading("Cargo.toml");
    expect(editing.path).toBe("Cargo.toml");
    expect(editing.kind).toBe("sheet");
    expect(editing.mode).toBe("edit");
  });

  it("Edit on a config file, then Markdown: the visual editor opens on it", async () => {
    showMock("Cargo.toml");
    chooseMode("edit");
    await flush();
    expect(editing.kind).toBe("sheet");
    await switchWhileLoading("README.md");
    expect(editing.path).toBe("README.md");
    expect(editing.kind).toBe("markdown");
    expect(editing.mode).toBe("edit");
  });

  it("Edit, then a text file without Edit opens in Source; the next Markdown file opens in Edit again", async () => {
    chooseMode("edit");
    await flush();
    await switchWhileLoading("notes/todo.txt");
    expect(editing.path).toBe("notes/todo.txt");
    expect(editing.kind).toBe("text");
    expect(editing.mode).toBe("source");
    await switchWhileLoading("docs/handbook.md");
    expect(editing.path).toBe("docs/handbook.md");
    expect(editing.kind).toBe("markdown");
    expect(editing.mode).toBe("edit");
  });

  it("Edit chosen while the next file loads opens no editor with the previous file's kind", async () => {
    shown.state.path = "Cargo.toml";
    expect(editBlockedReason()).toBe("noViewer");
    chooseMode("edit");
    await flush();
    expect(editing.path).toBeNull();
    expect(editing.mode).toBe("view");
    // its view lands: Edit is available and opens the sheet
    shown.state.view = mockViewOf("Cargo.toml");
    expect(editBlockedReason()).toBeNull();
    chooseMode("edit");
    await flush();
    expect(editing.path).toBe("Cargo.toml");
    expect(editing.kind).toBe("sheet");
    expect(editing.mode).toBe("edit");
  });
});
