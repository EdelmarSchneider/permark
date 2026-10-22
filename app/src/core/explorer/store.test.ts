import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import type { Api, ApiError, LinkUpdate, TreeChanged } from "../../api";
import { ImportFlow } from "./importFlow.svelte";
import { ExplorerStore, type ExplorerHost } from "./store.svelte";

type Mock = ReturnType<typeof createMockApi>;

type Fx = {
  api: Mock;
  store: ExplorerStore;
  failures: ApiError[];
  notes: string[];
  opened: string[];
  stop: () => void;
  setLink: (value: LinkUpdate) => void;
};

async function setup(link: LinkUpdate = "ask", wrap?: (api: Mock) => Api): Promise<Fx> {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  let setting = link;
  const failures: ApiError[] = [];
  const notes: string[] = [];
  const opened: string[] = [];
  const host: ExplorerHost = {
    fail: (error) => failures.push(error),
    note: (text) => notes.push(text),
    view: () => "main",
    linkUpdate: () => setting,
    openProject: (path) => opened.push(path),
  };
  const store = new ExplorerStore(wrap ? wrap(api) : api, host);
  const stop = await store.start();
  return { api, store, failures, notes, opened, stop, setLink: (v) => (setting = v) };
}

function paths(store: ExplorerStore): string[] {
  return store.nodes.map((n) => n.path);
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

// ---- loading and listening ---------------------------------------------------------------------------------------------

describe("loading", () => {
  it("opens the list with the project filter and shows the counts", async () => {
    const { store } = await setup();
    expect(store.load.status).toBe("ready");
    expect(paths(store)).toContain("README.md");
    expect(paths(store)).not.toContain(".github");
    expect(store.filter.show).toBe("docs-and-assets");
    expect(store.shownFiles).toBeGreaterThan(0);
    expect(store.totalFiles).toBeGreaterThanOrEqual(store.shownFiles);
  });

  it("remembers the filter and the open folders for the project", async () => {
    vi.useFakeTimers();
    const first = await setup();
    await first.store.setFilter({ ...first.store.filter, show: "everything" });
    await first.store.toggle("docs");
    await vi.advanceTimersByTimeAsync(500);
    expect((await first.api.projectSessionGet("explorer.expanded")).ok).toBe(true);
    // a second Explorer of the same window starts from what was remembered
    const again = new ExplorerStore(first.api, {
      fail: () => {},
      note: () => {},
      view: () => "main",
      linkUpdate: () => "ask",
      openProject: () => {},
    });
    await again.start();
    expect(again.filter.show).toBe("everything");
    expect(again.isOpen("docs")).toBe(true);
  });

  it("a delta that arrives while the list is loading is applied after it", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisTab");
    const slow: Api = {
      ...api,
      treeOpen: async (filter) => {
        const result = await api.treeOpen(filter);
        await gate;
        return result;
      },
    };
    const store = new ExplorerStore(slow, {
      fail: () => {},
      note: () => {},
      view: () => "main",
      linkUpdate: () => "never",
      openProject: () => {},
    });
    const started = store.start();
    await Promise.resolve();
    await Promise.resolve();
    expect(store.load.status).toBe("loading");
    // the backend tells of a new file after it sent the list, before the list was applied
    const created = await api.newEntry("", "early", "file");
    expect(created.ok).toBe(true);
    release();
    await started;
    expect(paths(store)).toContain("early.md");
  });

  it("a missed delta asks for the whole list again", async () => {
    const { api, store } = await setup();
    const before = store.nodes.length;
    await api.newEntry("", "one", "file");
    expect(store.nodes.length).toBe(before + 1);
    // a delta two ahead of what the window has: something was lost
    const event: TreeChanged = {
      window: "main",
      delta: {
        seq: 99,
        removed: [],
        added: [],
        renamed: [],
        totalFiles: 0,
        shownFiles: 0,
        bulk: false,
        filter: null,
      },
    };
    api.emit("tree-changed", event);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(store.load.status).toBe("ready");
    expect(paths(store)).toContain("one.md");
    // an old delta is ignored
    api.emit("tree-changed", { window: "main", delta: { ...event.delta, seq: 1 } });
    expect(paths(store)).toContain("one.md");
  });

  it("the teardown stops listening and closes the view", async () => {
    const { api, store, stop } = await setup();
    const close = vi.spyOn(api, "treeClose");
    stop();
    expect(close).toHaveBeenCalledOnce();
    const before = store.nodes.length;
    await api.newEntry("", "after", "file");
    expect(store.nodes.length).toBe(before);
  });

  it("a failed open shows as failed and can be retried", async () => {
    const api = createMockApi();
    let fail = true;
    const flaky: Api = {
      ...api,
      treeOpen: (filter) =>
        fail
          ? Promise.resolve({
              ok: false,
              error: { kind: "command", command: "tree_open", message: "no" },
            })
          : api.treeOpen(filter),
    };
    const store = new ExplorerStore(flaky, {
      fail: () => {},
      note: () => {},
      view: () => "main",
      linkUpdate: () => "ask",
      openProject: () => {},
    });
    await store.start();
    expect(store.load.status).toBe("failed");
    fail = false;
    await store.retry();
    expect(store.load.status).toBe("ready");
  });
});

// ---- opening folders, selecting -----------------------------------------------------------------------------------------

describe("rows and selection", () => {
  it("folders open and close, and collapse all closes every one", async () => {
    const { store } = await setup();
    expect(store.rows.some((r) => r.type === "node" && r.node.path === "docs/guide.md")).toBe(
      false,
    );
    await store.toggle("docs");
    expect(store.rows.some((r) => r.type === "node" && r.node.path === "docs/guide.md")).toBe(true);
    store.collapseAll();
    expect(store.rows.some((r) => r.type === "node" && r.node.path === "docs/guide.md")).toBe(
      false,
    );
  });

  it("click, Ctrl and Shift select the usual way", async () => {
    const { store } = await setup();
    await store.toggle("docs");
    const keys = store.rows.flatMap((r) => (r.type === "node" ? [r.node.path] : []));
    const [a, b, c, d] = keys;
    store.select(a ?? "", "single");
    store.select(c ?? "", "toggle");
    expect([...store.selection]).toEqual([a, c]);
    store.select(d ?? "", "range");
    expect([...store.selection]).toEqual(
      keys.slice(keys.indexOf(c ?? ""), keys.indexOf(d ?? "") + 1),
    );
    store.select(b ?? "", "single");
    expect([...store.selection]).toEqual([b]);
    store.select(b ?? "", "toggle");
    expect(store.selection.size).toBe(0);
  });

  it("arrows move the focus, extend with Shift, and fold folders", async () => {
    const { store } = await setup();
    store.moveTo("first", false);
    const first = store.focus;
    expect(first).not.toBeNull();
    store.move(1, false);
    expect(store.focus).not.toBe(first);
    store.move(-5, false);
    expect(store.focus).toBe(first);
    store.moveTo("last", true);
    expect(store.selection.size).toBeGreaterThan(1);
    // Right opens a folder, Left closes it, Left on a closed one goes to the parent
    store.select("docs", "single");
    await store.arrow("right");
    expect(store.isOpen("docs")).toBe(true);
    await store.arrow("right");
    expect(store.focus).toBe("docs/img");
    await store.arrow("left");
    expect(store.focus).toBe("docs");
    await store.arrow("left");
    expect(store.isOpen("docs")).toBe(false);
  });

  it("a file shows its document, a folder toggles", async () => {
    const { api, store } = await setup();
    await store.activate("README.md");
    const boot = await api.windowBoot();
    expect(boot.ok && boot.value.view.state === "showing" && boot.value.view.document).toBe(
      "README.md",
    );
    await store.activate("docs");
    expect(store.isOpen("docs")).toBe(true);
    await store.activate("ghost.md");
  });

  it("the open document is kept in the list and revealed", async () => {
    const { store } = await setup();
    await store.setActive("docs/img/logo.png");
    expect(store.isOpen("docs")).toBe(true);
    expect(store.isOpen("docs/img")).toBe(true);
    expect([...store.selection]).toEqual(["docs/img/logo.png"]);
    expect(store.scrollTo?.path).toBe("docs/img/logo.png");
    // collapsing and asking again brings it back
    store.collapseAll();
    store.revealActive();
    expect(store.isOpen("docs/img")).toBe(true);
    // none: nothing to reveal
    await store.setActive(null);
    store.collapseAll();
    store.revealActive();
    expect(store.isOpen("docs")).toBe(false);
  });
});

// ---- names ---------------------------------------------------------------------------------------------------------------

describe("renaming", () => {
  it("rename without link update applies at once and follows the new name", async () => {
    const f = await setup("never");
    f.store.select("notes/todo.txt", "single");
    f.store.beginRename();
    expect(f.store.edit).toMatchObject({
      mode: "rename",
      path: "notes/todo.txt",
      value: "todo.txt",
    });
    f.store.typeName("done.txt");
    await f.store.commitEdit();
    expect(f.store.edit.mode).toBe("none");
    expect(paths(f.store)).toContain("notes/done.txt");
    expect(paths(f.store)).not.toContain("notes/todo.txt");
    expect([...f.store.selection]).toEqual(["notes/done.txt"]);
    expect(f.notes.at(-1)).toContain("done.txt");
    expect(f.failures).toEqual([]);
  });

  it("with the setting 'ask' a move that changes links waits for the answer", async () => {
    const f = await setup("ask");
    f.store.select("notes/ideas.md", "single");
    f.store.beginRename();
    f.store.typeName("thoughts.md");
    await f.store.commitEdit();
    expect(f.store.dialog.status).toBe("asking");
    if (f.store.dialog.status === "asking") expect(f.store.dialog.plan.linkCount).toBe(2);
    expect(paths(f.store)).toContain("notes/ideas.md");
    await f.store.answerDialog("update");
    expect(f.store.dialog.status).toBe("closed");
    expect(paths(f.store)).toContain("notes/thoughts.md");
    expect(f.api.probe.project.files.get("README.md")).toContain("(notes/thoughts.md)");
    expect(f.notes.at(-1)).toMatch(/2 links updated/);
  });

  it("'Move only' leaves the links; cancel leaves everything", async () => {
    const f = await setup("ask");
    await f.store.requestMove([{ from: "notes/ideas.md", to: "notes/a.md" }]);
    await f.store.answerDialog("move");
    expect(f.api.probe.project.files.get("README.md")).toContain("(notes/ideas.md)");
    await f.store.requestMove([{ from: "notes/todo.txt", to: "notes/b.txt" }]);
    expect(f.store.dialog.status).toBe("closed");
    await f.store.requestMove([{ from: "docs/guide.md", to: "docs/g.md" }]);
    expect(f.store.dialog.status).toBe("asking");
    await f.store.answerDialog("cancel");
    expect(paths(f.store)).toContain("docs/guide.md");
  });

  it("'always' updates links without asking, 'never' does not even look", async () => {
    const always = await setup("always");
    await always.store.requestMove([{ from: "notes/ideas.md", to: "notes/i.md" }]);
    expect(always.store.dialog.status).toBe("closed");
    expect(always.api.probe.project.files.get("README.md")).toContain("(notes/i.md)");
    const never = await setup("never");
    const plan = vi.spyOn(never.api, "planMove");
    await never.store.requestMove([{ from: "notes/ideas.md", to: "notes/i.md" }]);
    expect(plan).toHaveBeenCalledWith([{ from: "notes/ideas.md", to: "notes/i.md" }], false);
    expect(never.api.probe.project.files.get("README.md")).toContain("(notes/ideas.md)");
  });

  it("a name that is taken or invalid keeps the field open with the reason", async () => {
    const f = await setup("never");
    f.store.beginRename("README.md");
    f.store.typeName("package.json");
    await f.store.commitEdit();
    expect(f.store.edit).toMatchObject({ mode: "rename", problem: "exists" });
    f.store.typeName("a/b.md");
    expect(f.store.edit).toMatchObject({ problem: "chars" });
    await f.store.commitEdit();
    expect(f.store.edit).toMatchObject({ mode: "rename", problem: "chars" });
    f.store.typeName("");
    expect(f.store.edit).toMatchObject({ problem: "empty" });
    f.store.typeName("ok.md ");
    expect(f.store.edit).toMatchObject({ problem: "edge" });
    f.store.typeName("README.md");
    await f.store.commitEdit();
    expect(f.store.edit.mode).toBe("none");
    expect(f.failures).toEqual([]);
  });

  it("escape cancels, and renaming to the same name does nothing", async () => {
    const f = await setup("never");
    const plan = vi.spyOn(f.api, "planMove");
    f.store.beginRename("README.md");
    f.store.cancelEdit();
    expect(f.store.edit.mode).toBe("none");
    f.store.beginRename("README.md");
    await f.store.commitEdit();
    expect(f.store.edit.mode).toBe("none");
    expect(plan).not.toHaveBeenCalled();
  });

  it("only one thing is renamed at a time", async () => {
    const f = await setup();
    f.store.select("README.md", "single");
    f.store.select("package.json", "toggle");
    f.store.beginRename();
    expect(f.store.edit.mode).toBe("none");
  });
});

describe("new files and folders", () => {
  it("a new folder gets a default name; a new file gets .md", async () => {
    const f = await setup();
    f.store.select("docs", "single");
    f.store.beginNew("folder");
    expect(f.store.edit).toMatchObject({
      mode: "new",
      kind: "folder",
      dir: "docs",
      value: "New folder",
    });
    expect(f.store.isOpen("docs")).toBe(true);
    await f.store.commitEdit();
    expect(paths(f.store)).toContain("docs/New folder");
    f.store.beginNew("file", "docs");
    f.store.typeName("draft");
    await f.store.commitEdit();
    expect(paths(f.store)).toContain("docs/draft.md");
    expect([...f.store.selection]).toEqual(["docs/draft.md"]);
  });

  it("a new entry next to a focused file goes in that file's folder; with nothing focused, in the project folder", async () => {
    const f = await setup();
    f.store.beginNew("file");
    expect(f.store.edit).toMatchObject({ mode: "new", dir: "" });
    f.store.cancelEdit();
    f.store.select("docs/guide.md", "single");
    f.store.beginNew("file");
    expect(f.store.edit).toMatchObject({ dir: "docs" });
  });

  it("a taken name is shown in the field", async () => {
    const f = await setup();
    f.store.beginNew("file", "");
    f.store.typeName("README.md");
    await f.store.commitEdit();
    expect(f.store.edit).toMatchObject({ mode: "new", problem: "exists" });
  });
});

// ---- delete, undo, drag --------------------------------------------------------------------------------------------------

describe("delete and undo", () => {
  it("deletes the selection (a folder takes its files along) and undo brings it back", async () => {
    const f = await setup();
    await f.store.toggle("notes");
    f.store.select("notes", "single");
    f.store.select("notes/todo.txt", "toggle");
    await f.store.remove();
    expect(paths(f.store).some((p) => p.startsWith("notes"))).toBe(false);
    expect(f.notes.at(-1)).toContain("notes");
    await f.store.undo();
    expect(paths(f.store)).toContain("notes/ideas.md");
    expect(f.notes.at(-1)).toBe("Undone.");
    await f.store.undo();
    expect(f.notes.at(-1)).toBe("Nothing to undo");
  });

  it("nothing selected, nothing deleted", async () => {
    const f = await setup();
    const spy = vi.spyOn(f.api, "deleteFiles");
    await f.store.remove();
    expect(spy).not.toHaveBeenCalled();
  });

  it("a refusal is told to the window", async () => {
    const f = await setup();
    await f.store.remove(["ghost.md"]);
    expect(f.failures).toHaveLength(1);
    expect(f.failures[0]).toMatchObject({ kind: "backend", error: { kind: "notFound" } });
  });
});

describe("drag and drop", () => {
  it("dropping on a folder moves the selection into it", async () => {
    const f = await setup("never");
    f.store.select("notes/todo.txt", "single");
    f.store.beginDrag(["notes/todo.txt"]);
    f.store.dragOver("docs");
    expect(f.store.drag).toMatchObject({ status: "dragging", over: "docs" });
    await f.store.endDrag();
    expect(f.store.drag.status).toBe("idle");
    expect(paths(f.store)).toContain("docs/todo.txt");
  });

  it("refused targets show no mark and do nothing", async () => {
    const f = await setup("never");
    const plan = vi.spyOn(f.api, "planMove");
    f.store.beginDrag(["docs"]);
    f.store.dragOver("docs/img");
    expect(f.store.drag).toMatchObject({ over: null });
    f.store.dragOver("notes");
    f.store.dragOver(null);
    await f.store.endDrag();
    expect(plan).not.toHaveBeenCalled();
    // cancelled mid-way
    f.store.beginDrag(["README.md"]);
    f.store.dragOver("docs");
    f.store.cancelDrag();
    await f.store.endDrag();
    expect(plan).not.toHaveBeenCalled();
  });

  it("dropping into the project folder uses an empty path", async () => {
    const f = await setup("never");
    f.store.beginDrag(["docs/guide.md"]);
    f.store.dragOver("");
    await f.store.endDrag();
    expect(paths(f.store)).toContain("guide.md");
  });

  it("dragOver outside a drag is ignored", async () => {
    const f = await setup();
    f.store.dragOver("docs");
    expect(f.store.drag.status).toBe("idle");
  });
});

// ---- outside PerMark -----------------------------------------------------------------------------------------------------

describe("copy and open", () => {
  it("copies the path, full or relative", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const f = await setup();
    await f.store.copyPath("docs/guide.md", false);
    await f.store.copyPath("docs/guide.md", true);
    expect(writeText).toHaveBeenNthCalledWith(1, "docs/guide.md");
    expect(writeText).toHaveBeenNthCalledWith(2, `${MOCK_FOLDER}/docs/guide.md`);
  });

  it("a clipboard that refuses is not an error", async () => {
    vi.stubGlobal("navigator", { clipboard: { writeText: () => Promise.reject(new Error("no")) } });
    const f = await setup();
    await expect(f.store.copyPath("a", false)).resolves.toBeUndefined();
    expect(f.failures).toEqual([]);
  });

  it("opens a folder as a project through the window, and files with the system", async () => {
    const f = await setup();
    await f.store.openAsProject("docs");
    expect(f.opened).toEqual([`${MOCK_FOLDER}/docs`]);
    await f.store.openExternal("README.md", "defaultApp");
    await f.store.openExternal("docs", "reveal");
    expect(f.api.probe.opened).toEqual([
      { path: "README.md", mode: "defaultApp" },
      { path: "docs", mode: "reveal" },
    ]);
  });
});

describe("the filter", () => {
  it("a new filter replaces the list; a project default comes back with reset", async () => {
    const f = await setup();
    const before = f.store.nodes.length;
    await f.store.setFilter({ ...f.store.filter, show: "everything" });
    expect(f.store.nodes.length).toBeGreaterThan(before - 1);
    expect(f.store.filter.show).toBe("everything");
    await f.store.resetFilter();
    expect(f.store.filter.show).toBe("docs-and-assets");
  });
});

// ---- the code offer ----------------------------------------------------------------------------------------------------

/** A project with `code` more code files than the mock's seed, in an Explorer that has not started yet. */
async function codeProject(code: number) {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  for (let i = 0; i < code; i++) api.probe.project.add(`src/part${i}.ts`, "export {};");
  const host: ExplorerHost = {
    fail: () => {},
    note: () => {},
    view: () => "main",
    linkUpdate: () => "ask",
    openProject: () => {},
  };
  const start = async () => {
    const store = new ExplorerStore(api, host);
    const stop = await store.start();
    return { store, stop };
  };
  return { api, start };
}

describe("the code offer", () => {
  it("is made once a project holds many code files the filter hides", async () => {
    const { start } = await codeProject(6);
    const { store } = await start();
    expect(store.filter.show).toBe("docs-and-assets");
    expect(paths(store)).not.toContain("src/part0.ts");
    // the seed's own src/main.rs and the six more
    expect(store.codeOffer).toEqual({ status: "offered", hidden: 7 });
  });

  it("is not made for a project with a few scripts", async () => {
    const { start } = await codeProject(1);
    const { store } = await start();
    expect(store.codeOffer).toEqual({ status: "none" });
  });

  it("'Show code files' shows them, remembers it for the project and never asks again", async () => {
    vi.useFakeTimers();
    const { start } = await codeProject(6);
    const first = await start();
    await first.store.acceptCodeOffer();
    expect(first.store.codeOffer).toEqual({ status: "none" });
    expect(first.store.filter.show).toBe("custom");
    expect(first.store.filter.categories).toContain("code");
    expect(paths(first.store)).toContain("src/part0.ts");
    expect(paths(first.store)).toContain("README.md");
    await vi.advanceTimersByTimeAsync(500);
    // the project opened again: code is shown and nobody asks
    const again = await start();
    expect(again.store.filter.categories).toContain("code");
    expect(paths(again.store)).toContain("src/part0.ts");
    expect(again.store.codeOffer).toEqual({ status: "none" });
    // hiding it by hand later does not bring the offer back
    await again.store.setFilter({ ...again.store.filter, show: "docs-and-assets" });
    expect(again.store.codeOffer).toEqual({ status: "none" });
    await again.store.resetFilter();
    expect(again.store.codeOffer).toEqual({ status: "none" });
  });

  it("'No thanks' keeps the default and never asks again for that project", async () => {
    const { api, start } = await codeProject(6);
    const first = await start();
    await first.store.declineCodeOffer();
    expect(first.store.codeOffer).toEqual({ status: "none" });
    expect(first.store.filter.show).toBe("docs-and-assets");
    expect(paths(first.store)).not.toContain("src/part0.ts");
    expect(await api.projectSessionGet("explorer.code-offer")).toEqual({
      ok: true,
      value: "declined",
    });
    const again = await start();
    expect(again.store.codeOffer).toEqual({ status: "none" });
    expect(again.store.filter.show).toBe("docs-and-assets");
  });

  it("is not made over a filter the person made", async () => {
    vi.useFakeTimers();
    const { start } = await codeProject(6);
    const first = await start();
    await first.store.setFilter({
      ...first.store.filter,
      show: "custom",
      categories: ["markdown"],
    });
    expect(first.store.codeOffer).toEqual({ status: "none" });
    await vi.advanceTimersByTimeAsync(500);
    const again = await start();
    expect(again.store.codeOffer).toEqual({ status: "none" });
  });

  it("ignores an answer it does not know in the session", async () => {
    const { api, start } = await codeProject(6);
    await api.projectSessionSet("explorer.code-offer", { surprise: true });
    const { store } = await start();
    expect(store.codeOffer.status).toBe("offered");
  });
});

// ---- a drag from outside -----------------------------------------------------------------------------------------------

describe("a drag from outside", () => {
  const P = { x: 1, y: 1 };
  const remembered = async (api: Mock): Promise<unknown> => {
    const got = await api.projectSessionGet("explorer.expanded");
    return got.ok ? got.value : null;
  };

  it("resting 700 ms on a closed folder opens it; a pointer that moves on restarts the wait", async () => {
    vi.useFakeTimers();
    const { store } = await setup();
    store.outsideOver("docs", P);
    expect(store.outside).toBe("docs");
    await vi.advanceTimersByTimeAsync(699);
    expect(store.isOpen("docs")).toBe(false);
    store.outsideOver("notes", P);
    await vi.advanceTimersByTimeAsync(400);
    store.outsideOver("docs", P);
    await vi.advanceTimersByTimeAsync(699);
    expect(store.isOpen("docs")).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(store.isOpen("docs")).toBe(true);
    expect(store.isOpen("notes")).toBe(false);
  });

  it("repeated over events on the same folder never restart the wait", async () => {
    vi.useFakeTimers();
    const { store } = await setup();
    for (let n = 0; n < 7; n++) {
      store.outsideOver("docs", { x: n, y: n });
      await vi.advanceTimersByTimeAsync(100);
    }
    expect(store.isOpen("docs")).toBe(true);
  });

  it("an open folder, a file's folder or the project folder: no wait", async () => {
    vi.useFakeTimers();
    const { store } = await setup();
    await store.toggle("docs");
    store.outsideOver("docs", P);
    store.outsideOver("", P);
    await vi.advanceTimersByTimeAsync(2000);
    expect(store.isOpen("docs")).toBe(true);
    expect(store.expanded.size).toBe(1);
  });

  it("leave, cancel or a drop elsewhere closes what the drag opened; the session never hears of it", async () => {
    vi.useFakeTimers();
    const { store, api } = await setup();
    await store.toggle("notes");
    await vi.advanceTimersByTimeAsync(500);
    const before = await remembered(api);
    store.outsideOver("docs", P);
    await vi.advanceTimersByTimeAsync(700);
    expect(store.isOpen("docs")).toBe(true);
    // something else writes the session meanwhile: the drag's folder is not in it
    await store.toggle("notes");
    await store.toggle("notes");
    await vi.advanceTimersByTimeAsync(500);
    expect(await remembered(api)).toEqual(before);
    store.outsideEnd(null);
    expect(store.isOpen("docs")).toBe(false);
    expect(store.isOpen("notes")).toBe(true);
    expect(store.outside).toBeNull();
    expect(store.outsidePoint).toBeNull();
  });

  it("a drop held for the question keeps the folders, the mark goes; the timer stops", async () => {
    vi.useFakeTimers();
    const { store } = await setup();
    store.outsideOver("docs", P);
    await vi.advanceTimersByTimeAsync(700);
    store.outsideOver("notes", P);
    store.outsideHold();
    await vi.advanceTimersByTimeAsync(2000);
    expect(store.isOpen("docs")).toBe(true);
    expect(store.isOpen("notes")).toBe(false);
    expect(store.outside).toBeNull();
  });

  it("an import into an opened folder keeps it (and the path to it) open, selects the new items, remembers", async () => {
    vi.useFakeTimers();
    const { store, api } = await setup();
    store.outsideOver("docs", P);
    await vi.advanceTimersByTimeAsync(700);
    store.outsideOver("docs/img", P);
    await vi.advanceTimersByTimeAsync(700);
    store.outsideHold();
    api.probe.project.add("docs/img/new.md", "x");
    api.probe.project.add("docs/img/new (2).md", "x");
    store.outsideEnd("docs/img");
    store.placed(["docs/img/new.md", "docs/img/new (2).md"]);
    expect(store.isOpen("docs")).toBe(true);
    expect(store.isOpen("docs/img")).toBe(true);
    await vi.advanceTimersByTimeAsync(500);
    expect(await remembered(api)).toEqual(expect.arrayContaining(["docs", "docs/img"]));
    expect(store.scrollTo?.path).toBe("docs/img/new.md");
  });

  it("items that reach the list after the import ended are selected when they arrive", async () => {
    const { store, api } = await setup();
    store.placed(["docs/late.md"]);
    expect(store.selection.size).toBe(0);
    api.probe.project.add("docs/late.md", "x");
    // the backend tells the window about the new file
    await api.newEntry("docs", "other.md", "file");
    await vi.waitFor(() => expect(store.selection.has("docs/late.md")).toBe(true));
  });

  it("teardown stops the wait", async () => {
    vi.useFakeTimers();
    const { store, stop } = await setup();
    store.outsideOver("docs", P);
    stop();
    await vi.advanceTimersByTimeAsync(2000);
    expect(store.isOpen("docs")).toBe(false);
  });
});

describe("undo of exactly one operation", () => {
  it("an import's Undo after a newer touching operation explains and changes nothing", async () => {
    const { store, api, notes } = await setup();
    const id = api.probe.project.recordImport(["docs/x.md"]);
    api.probe.project.add("docs/x.md", "x");
    // a newer operation on that very path
    api.probe.project.recordImport(["docs/x.md"]);
    await store.undoEntry(id);
    expect(notes.at(-1)).toBe(
      "This import can't be undone any more: its files were changed since.",
    );
    expect(api.probe.project.exists("docs/x.md")).toBe(true);
  });

  it("an untouched one is taken back, not the newest", async () => {
    const { store, api, notes } = await setup();
    api.probe.project.add("docs/x.md", "x");
    api.probe.project.add("docs/z.md", "z");
    const id = api.probe.project.recordImport(["docs/x.md"]);
    api.probe.project.recordImport(["docs/z.md"]);
    await store.undoEntry(id);
    expect(notes.at(-1)).toBe("Undone.");
    expect(api.probe.project.exists("docs/x.md")).toBe(false);
    expect(api.probe.project.exists("docs/z.md")).toBe(true);
  });
});

// ---- imported items show at once (director 2026-10-20: a copied file stayed out of the tree until a reload) ------------

describe("imported items in the tree", () => {
  /** The import question wired to this Explorer, as the window does. */
  function importer(api: Mock, store: ExplorerStore): ImportFlow {
    return new ImportFlow(api, {
      fail: () => {},
      note: () => {},
      cancelNote: () => () => {},
      action: () => {},
      project: () => "permark-docs",
      view: () => "main",
      shown: () => true,
      focus: () => {},
      end: (kept) => store.outsideEnd(kept),
      placed: (paths) => store.placed(paths),
      undo: (id) => store.undoEntry(id),
    });
  }

  const shown = (store: ExplorerStore, path: string): boolean =>
    store.rows.some((r) => r.type === "node" && r.node.path === path);

  it("another view's tree news is not this view's: its list and numbering stay its own", async () => {
    const { store, api } = await setup();
    const before = store.nodes.length;
    // another tab's project numbers its own news: whatever our next number is, one of these has it
    for (let seq = 1; seq <= 50; seq++) {
      api.emit("tree-changed", {
        window: "w-9",
        delta: {
          seq,
          removed: ["README.md"],
          added: [],
          renamed: [],
          totalFiles: 0,
          shownFiles: 0,
          bulk: false,
          filter: null,
        },
      } as TreeChanged);
    }
    expect(store.nodes.length).toBe(before);
    expect(store.nodes.some((n) => n.path === "README.md")).toBe(true);
    // our own news still lands
    const flow = importer(api, store);
    await flow.plan("docs", ["C:/out/new.md"], { x: 1, y: 1 });
    await flow.confirm("copy");
    await vi.waitFor(() => expect(shown(store, "docs/new.md")).toBe(true));
  });

  for (const mode of ["copy", "move"] as const) {
    for (const where of ["collapsed", "expanded", "auto-expanded", "root"] as const) {
      it(`${mode} into a ${where} folder: the new item is in the list and on screen right after`, async () => {
        vi.useFakeTimers();
        const { store, api } = await setup();
        const folder = where === "root" ? "" : "docs";
        if (where === "expanded") await store.toggle("docs");
        if (where === "auto-expanded") {
          store.outsideOver("docs", { x: 1, y: 1 });
          await vi.advanceTimersByTimeAsync(700);
          expect(store.isOpen("docs")).toBe(true);
        }
        store.outsideHold();
        vi.useRealTimers();
        const flow = importer(api, store);
        await flow.plan(folder, [`C:/out/${mode}-${where}.md`], { x: 1, y: 1 });
        await flow.confirm(mode);
        const path = folder === "" ? `${mode}-${where}.md` : `docs/${mode}-${where}.md`;
        expect(store.nodes.some((n) => n.path === path)).toBe(true);
        expect(shown(store, path)).toBe(true);
        expect(store.selection.has(path)).toBe(true);
        if (folder !== "") expect(store.isOpen("docs")).toBe(true);
      });
    }
  }
});
