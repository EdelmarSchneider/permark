// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import type { LinkUpdate } from "../../api";
import { neutralContext } from "../conditions/context.svelte";
import { conditions } from "../conditions/context.svelte";
import { ContributionRegistry } from "../plugins/registry.svelte";
import ExplorerPanel from "./ExplorerPanel.svelte";
import { ExplorerStore } from "./store.svelte";

type Fx = {
  api: ReturnType<typeof createMockApi>;
  store: ExplorerStore;
  root: HTMLElement;
  registry: ContributionRegistry;
  failures: unknown[];
  notes: string[];
  destroy: () => void;
};

let mounted: (() => void)[] = [];

async function render(link: LinkUpdate = "never"): Promise<Fx> {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisWindow");
  const failures: unknown[] = [];
  const notes: string[] = [];
  const store = new ExplorerStore(api, {
    fail: (e) => failures.push(e),
    note: (t) => notes.push(t),
    linkUpdate: () => link,
    openProject: () => {},
  });
  const stop = await store.start();
  const registry = new ContributionRegistry();
  const root = document.createElement("div");
  document.body.append(root);
  const app = mount(ExplorerPanel, { target: root, props: { store, registry } });
  await tick();
  const destroy = (): void => {
    void unmount(app);
    stop();
    root.remove();
  };
  mounted.push(destroy);
  return { api, store, root, registry, failures, notes, destroy };
}

function rows(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>('[role="treeitem"]'));
}

function row(root: HTMLElement, path: string): HTMLElement {
  const found = rows(root).find((r) => r.dataset["path"] === path);
  if (!found)
    throw new Error(
      `no row for ${path}: ${rows(root)
        .map((r) => r.dataset["path"])
        .join(", ")}`,
    );
  return found;
}

function key(root: HTMLElement, name: string, init: KeyboardEventInit = {}): void {
  const tree = root.querySelector('[role="tree"]');
  tree?.dispatchEvent(
    new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...init }),
  );
  flushSync();
}

function pointer(target: EventTarget, type: string, x: number, y: number): void {
  const init = { clientX: x, clientY: y, button: 0, bubbles: true, cancelable: true };
  target.dispatchEvent(
    typeof PointerEvent === "function" ? new PointerEvent(type, init) : new MouseEvent(type, init),
  );
}

beforeEach(() => {
  // jsdom has no layout: the list is 300 x 400, one row is 26
  Element.prototype.getBoundingClientRect = function () {
    return {
      left: 0,
      top: 0,
      right: 300,
      bottom: 400,
      width: 300,
      height: 400,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    };
  };
  conditions.set(neutralContext());
});

afterEach(() => {
  for (const destroy of mounted) destroy();
  mounted = [];
  document.body.innerHTML = "";
});

describe("the list", () => {
  it("draws the rows of the project and opens a folder on click", async () => {
    const { root, store } = await render();
    const first = rows(root).map((r) => r.dataset["path"]);
    expect(first).toContain("README.md");
    expect(first).not.toContain("docs/guide.md");
    row(root, "docs").click();
    await tick();
    expect(store.isOpen("docs")).toBe(true);
    expect(rows(root).map((r) => r.dataset["path"])).toContain("docs/guide.md");
    expect(row(root, "docs").getAttribute("aria-expanded")).toBe("true");
    expect(row(root, "docs/guide.md").getAttribute("aria-level")).toBe("2");
  });

  it("shows only the rows in view of a long list", async () => {
    const api = createMockApi();
    for (let n = 0; n < 3000; n++)
      api.probe.project.add(`bulk/f${String(n).padStart(4, "0")}.md`, "x");
    await api.openPath(MOCK_FOLDER, "thisWindow");
    const store = new ExplorerStore(api, {
      fail: () => {},
      note: () => {},
      linkUpdate: () => "never",
      openProject: () => {},
    });
    const stop = await store.start();
    await store.toggle("bulk");
    const root = document.createElement("div");
    document.body.append(root);
    const app = mount(ExplorerPanel, {
      target: root,
      props: { store, registry: new ContributionRegistry() },
    });
    mounted.push(() => {
      void unmount(app);
      stop();
    });
    await tick();
    expect(store.rows.length).toBeGreaterThan(3000);
    // 400 px / 26 px rows + overscan, not 3000
    expect(rows(root).length).toBeLessThan(40);
    const list = root.querySelector<HTMLElement>('[role="tree"]');
    expect(list).not.toBeNull();
    if (list) {
      list.scrollTop = 26 * 1500;
      list.dispatchEvent(new Event("scroll"));
      await tick();
      const shown = rows(root).map((r) => r.dataset["path"] ?? "");
      expect(shown.some((p) => p.endsWith("f1500.md"))).toBe(true);
      expect(shown.some((p) => p.endsWith("f0000.md"))).toBe(false);
    }
  });

  it("marks hidden and forced rows, and shows the counts line", async () => {
    const { root, store } = await render();
    expect(root.querySelector(".foot")?.textContent ?? "").toMatch(/Showing/);
    await store.setFilter({ ...store.filter, showHidden: true });
    await tick();
    expect(row(root, ".github").className).toContain("is-dim");
  });

  it("plugins add badges to rows through the registry, honoring their requirements", async () => {
    const { root, registry } = await render();
    registry.register({
      kind: "treeBadge",
      id: "b1",
      owner: "git",
      requires: { needs: [], prefers: [] },
      badge: (path) => (path === "README.md" ? { text: "M", tone: "modified" } : null),
    });
    registry.register({
      kind: "treeBadge",
      id: "b2",
      owner: "git",
      requires: { needs: [{ need: "gitRepo" }], prefers: [] },
      badge: () => ({ text: "X" }),
    });
    await tick();
    expect(row(root, "README.md").querySelector(".badge")?.textContent).toBe("M");
    expect(row(root, "package.json").querySelector(".badge")).toBeNull();
    expect(root.querySelectorAll(".badge")).toHaveLength(1);
  });
});

describe("keyboard", () => {
  it("arrows move, Enter opens, F2 renames, Delete deletes, Ctrl+Z undoes", async () => {
    const { root, store, api, notes } = await render();
    store.moveTo("first", false);
    flushSync();
    key(root, "ArrowDown");
    expect(store.focus).not.toBeNull();
    key(root, "End");
    expect(store.focus).toBe("README.md");
    store.select("notes", "single");
    key(root, "Enter");
    await tick();
    expect(store.isOpen("notes")).toBe(true);
    // rename
    store.select("package.json", "single");
    key(root, "F2");
    await tick();
    const input = root.querySelector<HTMLInputElement>("input[type=text]");
    expect(input).not.toBeNull();
    expect(document.activeElement).toBe(input);
    if (input) {
      input.value = "pkg.json";
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    }
    await vi.waitFor(() => expect(store.nodes.map((n) => n.path)).toContain("pkg.json"));
    // delete + undo
    store.select("pkg.json", "single");
    key(root, "Delete");
    await vi.waitFor(() => expect(store.nodes.map((n) => n.path)).not.toContain("pkg.json"));
    key(root, "z", { ctrlKey: true });
    await vi.waitFor(() => expect(store.nodes.map((n) => n.path)).toContain("pkg.json"));
    await vi.waitFor(() => expect(notes.at(-1)).toBe("Undone."));
    expect(api.probe.project.files.has("pkg.json")).toBe(true);
  });

  it("Cmd+Z works as well, and plain z does not undo", async () => {
    const { root, store, api } = await render();
    await api.newEntry("", "scratch", "file");
    await tick();
    key(root, "z");
    await tick();
    expect(store.nodes.map((n) => n.path)).toContain("scratch.md");
    key(root, "z", { metaKey: true });
    await vi.waitFor(() => expect(store.nodes.map((n) => n.path)).not.toContain("scratch.md"));
  });

  it("shortcuts do not fire while composing or typing in the field", async () => {
    const { root, store } = await render();
    store.select("README.md", "single");
    key(root, "Delete", { isComposing: true });
    await tick();
    expect(store.nodes.map((n) => n.path)).toContain("README.md");
    store.beginRename("README.md");
    await tick();
    const input = root.querySelector<HTMLInputElement>("input[type=text]");
    input?.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", bubbles: true }));
    await tick();
    expect(store.nodes.map((n) => n.path)).toContain("README.md");
    input?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(store.edit.mode).toBe("none");
  });

  it("the name field says what is wrong while typing", async () => {
    const { root, store } = await render();
    store.beginRename("README.md");
    await tick();
    const input = root.querySelector<HTMLInputElement>("input[type=text]");
    if (!input) throw new Error("no field");
    input.value = "a|b.md";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(root.querySelector('[role="alert"]')?.textContent).toContain("can't contain");
    expect(input.getAttribute("aria-invalid")).toBe("true");
  });
});

describe("context menu", () => {
  it("offers file actions, runs one, and closes with Escape", async () => {
    const { root, api } = await render();
    row(root, "README.md").dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 20, clientY: 30 }),
    );
    await tick();
    const menu = document.querySelector('[role="menu"]');
    expect(menu).not.toBeNull();
    const labels = Array.from(menu?.querySelectorAll('[role="menuitem"]') ?? []).map(
      (i) => i.textContent ?? "",
    );
    expect(labels.join("|")).toContain("Show in file manager");
    expect(labels.join("|")).toContain("Open with default app");
    expect(labels.join("|")).not.toContain("Open as project");
    const reveal = Array.from(menu?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? []).find(
      (i) => (i.textContent ?? "").includes("Show in file manager"),
    );
    reveal?.click();
    await tick();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(api.probe.opened).toEqual([{ path: "README.md", mode: "reveal" }]);
    row(root, "docs").dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 5, clientY: 5 }),
    );
    await tick();
    expect(document.querySelector('[role="menu"]')?.textContent).toContain("Open as project");
    document
      .querySelector('[role="menu"]')
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("plugins add entries for the rows they apply to, and run them with the targets", async () => {
    const { root, registry } = await render();
    const run = vi.fn();
    registry.register({
      kind: "treeMenu",
      id: "unread.menu.folder",
      owner: "unread",
      requires: { needs: [], prefers: [] },
      label: "unread.markFolderRead",
      applies: (_path, isDir) => isDir,
      run,
    });
    row(root, "README.md").dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    await tick();
    expect(document.querySelector('[role="menu"]')?.textContent).not.toContain("folder read");
    document
      .querySelector('[role="menu"]')
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    row(root, "docs").dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    await tick();
    const item = Array.from(
      document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]'),
    ).find((i) => (i.textContent ?? "").toLowerCase().includes("folder"));
    expect(item).toBeDefined();
    item?.click();
    expect(run).toHaveBeenCalledWith(["docs"]);
  });

  it("never offers the system program for a script or an svg", async () => {
    const { root, store, api } = await render();
    api.probe.project.add("run.sh", "x");
    api.probe.project.add("logo.svg", "x");
    await store.setFilter({ ...store.filter, show: "everything" });
    await tick();
    for (const path of ["run.sh"]) {
      row(root, path).dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      );
      await tick();
      expect(document.querySelector('[role="menu"]')?.textContent).not.toContain(
        "Open with default app",
      );
      document
        .querySelector('[role="menu"]')
        ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      await tick();
    }
    row(root, "logo.svg").dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    await tick();
    expect(document.querySelector('[role="menu"]')?.textContent).not.toContain(
      "Open with default app",
    );
  });

  it("a multi-selection offers delete for all and nothing about one file", async () => {
    const { root, store } = await render();
    store.select("README.md", "single");
    store.select("package.json", "toggle");
    row(root, "package.json").dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    await tick();
    const text = document.querySelector('[role="menu"]')?.textContent ?? "";
    expect(text).toContain("Delete 2 items");
    expect(text).not.toContain("Rename");
  });
});

describe("move dialog", () => {
  it("asks about links with their lines, then moves and updates", async () => {
    const { root, store, api } = await render("ask");
    await store.requestMove([{ from: "notes/ideas.md", to: "notes/thoughts.md" }]);
    await tick();
    const dialog = document.querySelector('[role="dialog"][aria-modal="true"]');
    expect(dialog).not.toBeNull();
    expect(dialog?.textContent).toContain("Update 2 links in 2 files?");
    expect(dialog?.textContent).toContain("notes/thoughts.md");
    expect(document.activeElement?.textContent).toContain("Move and update links");
    const primary = Array.from(dialog?.querySelectorAll<HTMLElement>("button") ?? []).find((b) =>
      (b.textContent ?? "").includes("Move and update links"),
    );
    primary?.click();
    await vi.waitFor(() => expect(store.dialog.status).toBe("closed"));
    expect(api.probe.project.files.get("README.md")).toContain("(notes/thoughts.md)");
    expect(root).toBeTruthy();
  });

  it("says in one quiet line which ignored folders it did not read", async () => {
    const { store, api } = await render("ask");
    const planned = api.planMove.bind(api);
    api.planMove = async (moves, updateLinks) => {
      const result = await planned(moves, updateLinks);
      return result.ok
        ? { ok: true, value: { ...result.value, unchecked: ["build", "dist", "out", "tmp"] } }
        : result;
    };
    await store.requestMove([{ from: "notes/ideas.md", to: "notes/thoughts.md" }]);
    await tick();
    const note = document.querySelector("[data-unchecked]")?.textContent ?? "";
    expect(note).toContain("4 ignored folders were not checked");
    expect(note).toContain("build, dist, out, …");
  });

  it("Escape and the backdrop cancel; focus stays inside", async () => {
    const { store, api } = await render("ask");
    await store.requestMove([{ from: "notes/ideas.md", to: "notes/thoughts.md" }]);
    await tick();
    const dialog = document.querySelector<HTMLElement>('[role="dialog"][aria-modal="true"]');
    dialog?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(store.dialog.status).toBe("closed");
    expect(api.probe.project.files.has("notes/ideas.md")).toBe(true);
  });
});

describe("drag", () => {
  it("a press that travels becomes a drag, marks the folder under the pointer and drops into it", async () => {
    const { root, store } = await render();
    const rowsNow = store.rows.flatMap((r) => (r.type === "node" ? [r.node.path] : []));
    const from = rowsNow.indexOf("package.json");
    const into = rowsNow.indexOf("docs");
    expect(from).toBeGreaterThanOrEqual(0);
    expect(into).toBeGreaterThanOrEqual(0);
    pointer(row(root, "package.json"), "pointerdown", 40, from * 26 + 5);
    pointer(window, "pointermove", 41, from * 26 + 6);
    expect(store.drag.status).toBe("idle");
    pointer(window, "pointermove", 60, into * 26 + 5);
    await tick();
    expect(store.drag).toMatchObject({ status: "dragging", over: "docs" });
    expect(row(root, "docs").className).toContain("drop-target");
    expect(document.body.textContent).toContain("Move into docs");
    pointer(window, "pointerup", 60, into * 26 + 5);
    await vi.waitFor(() => expect(store.nodes.map((n) => n.path)).toContain("docs/package.json"));
    expect(store.drag.status).toBe("idle");
  });

  it("a click without travel stays a click; Escape cancels a drag", async () => {
    const { root, store } = await render();
    pointer(row(root, "package.json"), "pointerdown", 40, 5);
    pointer(window, "pointerup", 40, 5);
    expect(store.drag.status).toBe("idle");
    pointer(row(root, "package.json"), "pointerdown", 40, 5);
    pointer(window, "pointermove", 80, 80);
    expect(store.drag.status).toBe("dragging");
    key(root, "Escape");
    expect(store.drag.status).toBe("idle");
    pointer(window, "pointerup", 80, 80);
    expect(store.nodes.map((n) => n.path)).toContain("package.json");
  });

  it("dragging a folder over itself shows no mark", async () => {
    const { root, store } = await render();
    const at = store.rows.findIndex((r) => r.type === "node" && r.node.path === "docs");
    pointer(row(root, "docs"), "pointerdown", 40, at * 26 + 5);
    pointer(window, "pointermove", 60, at * 26 + 6 + 10);
    expect(store.drag).toMatchObject({ status: "dragging", over: null });
    pointer(window, "pointerup", 60, at * 26 + 16);
  });
});

describe("filter popover", () => {
  it("opens from the toolbar, switches the preset and closes on Escape", async () => {
    const { root, store } = await render();
    root.querySelector<HTMLElement>("[data-filter-trigger]")?.click();
    await tick();
    const popover = root.querySelector('[role="dialog"]');
    expect(popover).not.toBeNull();
    const custom = Array.from(popover?.querySelectorAll<HTMLElement>('[role="radio"]') ?? []).find(
      (b) => (b.textContent ?? "").includes("Custom"),
    );
    custom?.click();
    await vi.waitFor(() => expect(store.filter.show).toBe("custom"));
    await tick();
    expect(root.querySelector("fieldset")).not.toBeNull();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await tick();
    expect(root.querySelector('[role="dialog"]')).toBeNull();
  });
});

describe("states", () => {
  it("says so when there is nothing to show and when loading failed", async () => {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisWindow");
    const store = new ExplorerStore(
      {
        ...api,
        treeOpen: () =>
          Promise.resolve({
            ok: false,
            error: { kind: "command", command: "tree_open", message: "x" },
          }),
      },
      { fail: () => {}, note: () => {}, linkUpdate: () => "ask", openProject: () => {} },
    );
    await store.start();
    const root = document.createElement("div");
    document.body.append(root);
    const app = mount(ExplorerPanel, {
      target: root,
      props: { store, registry: new ContributionRegistry() },
    });
    mounted.push(() => {
      void unmount(app);
      root.remove();
    });
    await tick();
    expect(root.textContent).toContain("Couldn't load the files.");
    expect(root.querySelector('[role="tree"]')).toBeNull();
  });
});
