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
const { chooseMode, openRemembered } = await import("./choose");

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
