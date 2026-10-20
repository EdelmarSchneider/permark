import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import type { Api, ApiError, LinkUpdate, TreeChanged } from "../../api";
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
  await api.openPath(MOCK_FOLDER, "thisWindow");
  let setting = link;
  const failures: ApiError[] = [];
  const notes: string[] = [];
  const opened: string[] = [];
  const host: ExplorerHost = {
    fail: (error) => failures.push(error),
    note: (text) => notes.push(text),
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
    await api.openPath(MOCK_FOLDER, "thisWindow");
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
  await api.openPath(MOCK_FOLDER, "thisWindow");
  for (let i = 0; i < code; i++) api.probe.project.add(`src/part${i}.ts`, "export {};");
  const host: ExplorerHost = {
    fail: () => {},
    note: () => {},
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
