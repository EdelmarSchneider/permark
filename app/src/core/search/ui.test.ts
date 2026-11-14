// @vitest-environment jsdom
// The Search everywhere box, the Search panel and the keyboard dialog, mounted against the mock backend.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};
// jsdom has no layout: a region's size watcher sees nothing
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};

const { api } = await import("../../api");
const { default: Finder } = await import("./Finder.svelte");
const { default: SearchPanel } = await import("./SearchPanel.svelte");
const { default: KeymapDialog } = await import("../shell/KeymapDialog.svelte");
const { default: ViewRegion } = await import("../shell/ViewRegion.svelte");
const { FILES, SEARCH } = await import("../shell/arrangement");
const { arrangement } = await import("../shell/arrangement.svelte");
const { selectView, showView } = await import("../shell/sides");
const { ui } = await import("../shell/ui.svelte");
const { windowStore } = await import("../window.svelte");
const { contributions, syncPlugins } = await import("../plugins/instance.svelte");
const { registerCoreCommands } = await import("../commands/core");
const { documents } = await import("../doc/document.svelte");
const { recentFiles } = await import("./recent.svelte");
const { conditions } = await import("../conditions/context.svelte");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { t } = await import("../i18n");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let unregister: (() => void) | null = null;
const store = windowStore;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function type(input: HTMLInputElement, text: string): void {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function press(el: Element | Window, key: string, init: KeyboardEventInit = {}): void {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
}

const rowTexts = (): string[] =>
  Array.from(target.querySelectorAll(".row")).map(
    (r) => r.textContent?.replace(/\s+/g, " ").trim() ?? "",
  );

async function open(): Promise<void> {
  await mock.openPath(MOCK_FOLDER, "thisTab");
  if (!started) {
    await store.start();
    started = true;
  }
}

beforeEach(async () => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  ui.closeFinder();
  ui.closeKeyboard();
  selectView(FILES);
  unregister = registerCoreCommands(contributions);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  unregister?.();
  documents.setWorkspace(null);
  syncPlugins([]);
  // the person's keys are settings of the (shared) mock: put them back
  for (const command of ["edit.save", "search.quickOpen"]) {
    await mock.settingsSet({ key: "keymap", value: { command, combo: null } });
  }
});

describe("the command palette", () => {
  it("lists the commands, dims the ones that cannot run and says why", async () => {
    // no folder open: the project commands are unavailable
    conditions.set({ ...conditions.current, workspace: "welcome", plugins: [] });
    ui.openFinder("commands");
    component = mount(Finder, { target, props: { preset: "commands" } });
    flushSync();
    await tick();
    const rows = Array.from(target.querySelectorAll<HTMLElement>(".row"));
    const find = (text: string): HTMLElement | undefined =>
      rows.find((r) => r.textContent?.includes(text));
    expect(find("Open folder")?.classList.contains("dim")).toBe(false);
    const explorer = find("Show Explorer");
    expect(explorer?.classList.contains("dim")).toBe(true);
    expect(explorer?.textContent).toContain("Open a folder first.");
    // the commands that open the box are not listed; the keys are
    expect(rows.some((r) => r.textContent?.includes("Command palette"))).toBe(false);
    expect(find("Open folder")?.querySelector("kbd")?.textContent).toBe("Ctrl+O");
  });

  it("offers the remedy of a plugin condition and carries it out", async () => {
    const off = contributions.register({
      kind: "command",
      owner: "git",
      id: "git.log",
      label: "cmd.save",
      run: () => {},
      requires: { needs: [{ need: "plugin", plugin: "git" }], prefers: [] },
    });
    await open();
    conditions.set({ ...conditions.current, plugins: [] });
    component = mount(Finder, { target, props: { preset: "commands" } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    type(input, "save");
    flushSync();
    await tick();
    const row = Array.from(target.querySelectorAll<HTMLElement>(".row")).find((r) =>
      r.textContent?.includes("The plugin for this is turned off."),
    );
    expect(row, rowTexts().join(" | ")).toBeDefined();
    const button = row?.querySelector<HTMLButtonElement>(".remedy");
    expect(button?.textContent).toBe("Turn the plugin on");
    button?.click();
    await until(() => store.keymap !== undefined, "settings round trip");
    const settings = await mock.settingsGet();
    expect(settings.ok && settings.value.plugins["git"]).toBe(true);
    off();
  });

  it("runs the chosen command on Enter and closes", async () => {
    await open();
    ui.openFinder("commands");
    component = mount(Finder, { target, props: { preset: "commands" } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    type(input, "settings");
    flushSync();
    await tick();
    press(input, "Enter");
    expect(ui.settings).toBe(true);
    expect(ui.finder.status).toBe("closed");
    ui.closeSettingsPopover();
  });
});

describe("typing in the box", () => {
  // the runs of a label change length with every key: the text must always be the label, the marks the typed words
  it("keeps every row's text whole and marks what is typed, key by key", async () => {
    await open();
    // an initialized project does not offer "Initialize PerMark…" (its letters would match too)
    await mock.projectInit(MOCK_FOLDER);
    await tick();
    ui.openFinder("commands");
    component = mount(Finder, { target, props: { preset: "commands" } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    const labels = new Set(["Zoom in", "Zoom out", "Reset zoom"]);
    for (const typed of ["z", "zo", "zoo", "zoom"]) {
      type(input, typed);
      flushSync();
      await tick();
      const rows = Array.from(target.querySelectorAll(".row .strong"));
      expect(rows.map((r) => r.textContent?.trim()).sort()).toEqual([...labels].sort());
      for (const row of rows) {
        const marked = Array.from(row.querySelectorAll(".hit"))
          .map((m) => m.textContent)
          .join("")
          .toLowerCase();
        expect(marked, `${row.textContent} for ${typed}`).toBe(typed);
      }
    }
    // and back: fewer words, other rows
    type(input, "reset");
    flushSync();
    await tick();
    const only = Array.from(target.querySelectorAll(".row .strong"));
    // the best match first (a long label elsewhere may hold the letters too)
    expect(only[0]?.textContent?.trim()).toBe("Reset zoom");
    expect(only[0]?.querySelector(".hit")?.textContent).toBe("Reset");
  });
});

describe("Quick open and Search everywhere", () => {
  it("finds a file by a few letters and opens it on Enter", async () => {
    await open();
    ui.openFinder("quick");
    component = mount(Finder, { target, props: { preset: "quick" } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    expect(document.activeElement).toBe(input);
    type(input, "gui");
    await until(() => rowTexts().some((t) => t.includes("guide.md")), "the file is listed");
    press(input, "Enter");
    await vi.waitFor(
      async () => {
        const boot = await mock.windowBoot();
        const shown = boot.ok && boot.value.view.state === "showing" && boot.value.view.document;
        expect(shown).toBe("docs/guide.md");
      },
      { timeout: 10_000, interval: 10 },
    );
    expect(ui.finder.status).toBe("closed");
  });

  it("draws a hidden file dimmed with its mark, after the files that are not hidden", async () => {
    await open();
    ui.openFinder("quick");
    component = mount(Finder, { target, props: { preset: "quick" } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    type(input, "ci");
    await until(() => rowTexts().some((t) => t.includes("ci.yml")), "the hidden file is listed");
    const rows = [...target.querySelectorAll<HTMLElement>("[role=option]")];
    const hidden = rows.findIndex((r) => r.textContent?.includes("ci.yml"));
    expect(rows[hidden]?.classList.contains("hidden-path")).toBe(true);
    expect(rows[hidden]?.querySelector(".mark")?.textContent).toBe("hidden");
    // every row after it is hidden too; none before it is
    expect(rows.slice(0, hidden).some((r) => r.classList.contains("hidden-path"))).toBe(false);
    expect(rows.slice(hidden).every((r) => r.classList.contains("hidden-path"))).toBe(true);
  });

  it("lists the recently opened files first", async () => {
    await open();
    recentFiles.visit("notes/ideas.md");
    ui.openFinder("quick");
    component = mount(Finder, { target, props: { preset: "quick" } });
    flushSync();
    await until(() => target.querySelectorAll(".group").length > 0, "recent groups");
    expect(target.querySelector(".group")?.textContent).toBe("Recently opened");
    expect(rowTexts()[0]).toContain("ideas.md");
  });

  it("Tab walks the scopes and Escape closes", async () => {
    await open();
    ui.openFinder("everywhere");
    component = mount(Finder, { target, props: { preset: "everywhere" } });
    flushSync();
    const tabs = (): string[] =>
      Array.from(target.querySelectorAll('[role="tab"]')).map(
        (t) => `${t.textContent}${t.getAttribute("aria-selected") === "true" ? "*" : ""}`,
      );
    expect(tabs()).toEqual(["All*", "Files", "Sections", "Commands"]);
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    press(input, "Tab");
    flushSync();
    expect(tabs()).toEqual(["All", "Files*", "Sections", "Commands"]);
    press(input, "Tab", { shiftKey: true });
    press(input, "Tab", { shiftKey: true });
    flushSync();
    expect(tabs()).toEqual(["All", "Files", "Sections", "Commands*"]);
    press(input, "Escape");
    expect(ui.finder.status).toBe("closed");
  });

  it("says when nothing matches", async () => {
    await open();
    ui.openFinder("quick");
    component = mount(Finder, { target, props: { preset: "quick" } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    type(input, "zzzzqq");
    await until(
      () => target.querySelector(".note")?.textContent?.includes("Nothing matches") === true,
      "no match note",
    );
  });

  it("an IME composition never moves the selection or opens a file", async () => {
    await open();
    ui.openFinder("quick");
    component = mount(Finder, { target, props: { preset: "quick" } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    type(input, "guide");
    await until(() => rowTexts().length > 0, "rows");
    press(input, "Enter", { isComposing: true });
    press(input, "ArrowDown", { isComposing: true });
    await tick();
    expect(ui.finder.status).toBe("open");
  });
});

describe("the Search panel", () => {
  it("shows the documents that hold the words and opens the section on a click", async () => {
    await open();
    showView(SEARCH);
    component = mount(SearchPanel, { target, props: { root: MOCK_FOLDER } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    expect(target.textContent).toContain("Search the text of every doc");
    type(input, "ideas");
    await until(() => target.querySelectorAll(".hit").length > 0, "hits");
    expect(target.querySelector(".file")?.textContent).toContain("ideas.md");
    expect(target.querySelector(".summary")?.textContent).toContain("file");
    // marks are elements, never HTML from the document
    expect(target.querySelector(".snippet .hit")).not.toBeNull();
    const navigate = vi.spyOn(documents, "navigate").mockImplementation(() => {});
    target.querySelector<HTMLElement>(".hit")?.click();
    expect(navigate).toHaveBeenCalledWith("notes/ideas.md", "ideas", 1);
    navigate.mockRestore();
  });

  it("clears on Escape, says when nothing matches, offers the silenced switch only with its plugin", async () => {
    await open();
    component = mount(SearchPanel, { target, props: { root: MOCK_FOLDER } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    conditions.set({ ...conditions.current, plugins: [] });
    flushSync();
    expect(target.querySelector("label.option")).toBeNull();
    type(input, "qqqqzzzz");
    await until(() => target.textContent?.includes("No results for") === true, "none");
    press(input, "Escape");
    flushSync();
    expect(input.value).toBe("");
    conditions.set({ ...conditions.current, plugins: ["quiet"] });
    flushSync();
    expect(target.querySelector("label.option")?.textContent).toContain("Include silenced");
    conditions.set({ ...conditions.current, plugins: [] });
  });

  it("takes the cursor once per request: the arrangement worked out again never takes the keyboard back", async () => {
    await open();
    conditions.set({ ...conditions.current, workspace: "project" });
    showView(SEARCH);
    component = mount(SearchPanel, { target, props: { root: MOCK_FOLDER } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    expect(arrangement.isSelected(SEARCH)).toBe(true);
    expect(document.activeElement).toBe(input);
    // the keyboard goes elsewhere (a view's options after a move)
    const other = document.createElement("button");
    document.body.append(other);
    other.focus();
    // the arrangement comes back from the settings a moment after a move: the same places, new objects
    arrangement.arrange(JSON.parse(JSON.stringify(arrangement.saved)) as typeof arrangement.saved);
    flushSync();
    expect(arrangement.isSelected(SEARCH)).toBe(true);
    expect(document.activeElement).toBe(other);
    // the context of the window changes (git answered, a plugin switched): the same
    conditions.set({ ...conditions.current });
    flushSync();
    expect(document.activeElement).toBe(other);
    // asked for again (its command, its tab): the box takes the cursor
    showView(SEARCH);
    flushSync();
    expect(document.activeElement).toBe(input);
    other.remove();
  });

  it("reports the index being built", async () => {
    await open();
    component = mount(SearchPanel, { target, props: { root: MOCK_FOLDER } });
    flushSync();
    await tick();
    mock.emit("index-progress", { root: MOCK_FOLDER, phase: "building", done: 3, total: 40 });
    await until(() => target.textContent?.includes("Indexing 3 of 40 files") === true, "progress");
    mock.emit("index-progress", { root: MOCK_FOLDER, phase: "ready", done: 40, total: 40 });
    await until(() => target.textContent?.includes("Indexing") !== true, "progress gone");
  });
});

describe("the sidebar tabs", () => {
  /** The left region of a project with no plugin tab: Files | Search. */
  function mountSidebar(): void {
    syncPlugins([]);
    conditions.set({ ...conditions.current, workspace: "project", plugins: [] });
    component = mount(ViewRegion, { target, props: { region: "left" } });
    flushSync();
  }
  const shown = (): string[] => [FILES, SEARCH].filter((id) => arrangement.isSelected(id));

  it("switch between Files and Search", async () => {
    mountSidebar();
    const [files, search] = Array.from(target.querySelectorAll<HTMLElement>('[role="tab"]'));
    expect(files?.getAttribute("aria-selected")).toBe("true");
    search?.click();
    flushSync();
    expect(shown()).toEqual([SEARCH]);
    expect(search?.getAttribute("aria-selected")).toBe("true");
    files?.click();
    flushSync();
    expect(shown()).toEqual([FILES]);
  });

  it("are one Tab stop: arrows move between the tabs, Enter | Space (a click) selects", async () => {
    mountSidebar();
    const list = target.querySelector<HTMLElement>('[role="tablist"]');
    expect(list?.getAttribute("aria-label")).toBe("Sidebar views");
    const tabs = (): HTMLElement[] => [...target.querySelectorAll<HTMLElement>('[role="tab"]')];
    const [files, search] = tabs();
    expect(tabs().map((tab) => tab.tabIndex)).toEqual([0, -1]);
    files?.focus();
    const press = (key: string): void => {
      document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
      flushSync();
    };
    press("ArrowRight");
    expect(document.activeElement).toBe(search);
    expect(tabs().map((tab) => tab.tabIndex)).toEqual([-1, 0]);
    // the focus moved, the view did not: the person selects it
    expect(shown()).toEqual([FILES]);
    press("ArrowRight");
    expect(document.activeElement).toBe(files);
    press("End");
    expect(document.activeElement).toBe(search);
    press("Home");
    expect(document.activeElement).toBe(files);
    press("ArrowLeft");
    expect(document.activeElement).toBe(search);
    search?.click();
    flushSync();
    expect(shown()).toEqual([SEARCH]);
    expect(tabs().map((tab) => tab.tabIndex)).toEqual([-1, 0]);
  });
});

describe("the keyboard dialog", () => {
  async function mountDialog(): Promise<void> {
    await open();
    ui.openKeyboard();
    component = mount(KeymapDialog, { target, props: { store } });
    flushSync();
    await tick();
  }

  const rowOf = (name: string): HTMLElement => {
    const row = Array.from(target.querySelectorAll<HTMLElement>(".row")).find(
      (r) => r.querySelector(".label")?.textContent === name,
    );
    if (!row) throw new Error(`no row ${name}`);
    return row;
  };
  const button = (row: HTMLElement, text: string): HTMLButtonElement | undefined =>
    Array.from(row.querySelectorAll("button")).find((b) => b.textContent?.trim() === text);

  it("lists every command with its keys, including the ones the palette does not show", async () => {
    await mountDialog();
    expect(rowOf("Save").querySelector("kbd")?.textContent).toBe("Ctrl+S");
    expect(rowOf("Go to file").querySelector("kbd")?.textContent).toBe("Ctrl+P");
    expect(rowOf("Command palette").querySelector("kbd")?.textContent).toBe("Ctrl+Shift+P");
    expect(rowOf("Keyboard shortcuts…").textContent).toContain("No keys");
  });

  it("Tab | Shift+Tab stay inside the dialog", async () => {
    await mountDialog();
    const dialog = target.querySelector<HTMLElement>("[role=dialog]");
    const search = dialog?.querySelector<HTMLInputElement>("input[type=search]");
    const close = Array.from(dialog?.querySelectorAll("button") ?? []).find(
      (b) => b.textContent?.trim() === "Close",
    );
    if (!search || !close) throw new Error("no dialog");
    expect(document.activeElement).toBe(search);
    close.focus();
    press(close, "Tab");
    expect(document.activeElement).toBe(search);
    press(search, "Tab", { shiftKey: true });
    expect(document.activeElement).toBe(close);
  });

  it("takes the next key press as the new keys and keeps it in the settings", async () => {
    await mountDialog();
    button(rowOf("Save"), "Change")?.click();
    flushSync();
    expect(rowOf("Save").textContent).toContain("Press the new keys");
    // keys no command holds
    press(window, "k", { ctrlKey: true, code: "KeyK" });
    await until(() => store.keymap["edit.save"] === "Mod+k", "stored");
    flushSync();
    expect(rowOf("Save").querySelector("kbd")?.textContent).toBe("Ctrl+K");
    expect(button(rowOf("Save"), "Reset")).toBeDefined();
    button(rowOf("Save"), "Reset")?.click();
    await until(() => store.keymap["edit.save"] === undefined, "back to the default");
    flushSync();
    expect(rowOf("Save").querySelector("kbd")?.textContent).toBe("Ctrl+S");
  });

  it("says who holds a key before it is taken; the person can keep it anyway", async () => {
    await mountDialog();
    button(rowOf("Save"), "Change")?.click();
    flushSync();
    press(window, "p", { ctrlKey: true, code: "KeyP" });
    flushSync();
    const ask = rowOf("Save").querySelector(".ask");
    expect(ask?.textContent).toContain("Ctrl+P is also used by Go to file");
    expect(store.keymap["edit.save"]).toBeUndefined();
    Array.from(ask?.querySelectorAll("button") ?? [])
      .find((b) => b.textContent?.trim() === "Use anyway")
      ?.click();
    await until(() => store.keymap["edit.save"] === "Mod+p", "stored");
    flushSync();
    expect(rowOf("Save").textContent).toContain("Same keys as Go to file");
    expect(rowOf("Go to file").textContent).toContain("Same keys as Save");
    // Escape while listening cancels, Backspace unbinds
    button(rowOf("Go to file"), "Change")?.click();
    flushSync();
    press(window, "Escape");
    flushSync();
    expect(rowOf("Go to file").textContent).not.toContain("Press the new keys");
    button(rowOf("Go to file"), "Change")?.click();
    flushSync();
    press(window, "Backspace");
    await until(() => store.keymap["search.quickOpen"] === "", "unbound");
    flushSync();
    expect(rowOf("Go to file").textContent).toContain("No keys");
    // reset all
    Array.from(target.querySelectorAll("footer button"))
      .find((b) => b.textContent?.trim() === "Reset all")
      ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await until(() => Object.keys(store.keymap).length === 0, "all reset");
  });

  it("filters by words", async () => {
    await mountDialog();
    const filter = target.querySelector<HTMLInputElement>("input.filter");
    if (!filter) throw new Error("no filter");
    type(filter, "zoom");
    flushSync();
    const labels = Array.from(target.querySelectorAll(".label")).map((l) => l.textContent);
    expect(labels).toEqual(["Zoom in", "Zoom out", "Reset zoom"]);
    expect(NO_REQUIREMENT.needs).toEqual([]);
  });
});

describe("marks on the files of search results (UX regressions #10)", () => {
  const badgeTitles = (row: Element | undefined): string[] =>
    Array.from(row?.querySelectorAll(".badge") ?? []).map((b) => b.getAttribute("title") ?? "");

  it("shows the unread dot and the git letter as the file list does, and nothing with the plugins off", async () => {
    await open();
    conditions.set({ ...conditions.current, git: { status: "repo" } as never });
    syncPlugins(["unread", "git"]);
    ui.openFinder("quick");
    component = mount(Finder, { target, props: { preset: "quick" } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    type(input, "guide");
    const guideRow = (): Element | undefined =>
      Array.from(target.querySelectorAll("[role=option]")).find((r) =>
        r.textContent?.includes("guide.md"),
      );
    await until(() => badgeTitles(guideRow()).length === 2, "two marks on the row");
    const titles = badgeTitles(guideRow());
    expect(titles).toContain("Unread");
    expect(
      titles.some((title) => /modified/i.test(title)),
      titles.join(" | "),
    ).toBe(true);
    // the words are in the tooltip; the mark is not color only
    expect(guideRow()?.querySelector(".badge.tone-unread .mark")?.textContent).toBe("•");
    // screen readers hear the words, not the raw marks (UX review restore-b4 F1)
    expect(guideRow()?.querySelector(".badge.tone-unread .mark")?.getAttribute("aria-hidden")).toBe(
      "true",
    );
    expect(guideRow()?.querySelector(".badge.tone-unread .sr-only")?.textContent).toBe(", Unread");
    syncPlugins([]);
    await until(() => badgeTitles(guideRow()).length === 0, "the marks go with their plugins");
  });

  it("puts the same marks on the file rows of the Search panel", async () => {
    await open();
    syncPlugins(["unread"]);
    component = mount(SearchPanel, { target, props: { root: MOCK_FOLDER } });
    flushSync();
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    type(input, "guide");
    const fileRow = (): Element | undefined =>
      Array.from(target.querySelectorAll(".file")).find((r) => r.textContent?.includes("guide.md"));
    await until(() => badgeTitles(fileRow()).includes("Unread"), "the unread dot");
  });
});

describe("Go to a recent place (UX regressions #12)", () => {
  it("lists the trail newest first, says where you are, narrows by typing and goes on Enter", async () => {
    await open();
    documents.trail.clear();
    for (const path of ["README.md", "docs/guide.md", "notes/ideas.md"]) {
      documents.trail.visit({ path, anchor: null });
    }
    documents.trail.visit({ path: "docs/guide.md", anchor: "setup" });
    const command = contributions.of("command").find((c) => c.id === "trail.show");
    expect(command && t(command.label)).toBe("Go to a recent place...");
    expect(command?.when?.()).toBe(true);
    command?.run();
    expect(ui.finder).toMatchObject({ status: "open", preset: "trail" });
    component = mount(Finder, { target, props: { preset: "trail" } });
    flushSync();
    await tick();
    expect(target.querySelector(".box")?.getAttribute("aria-label")).toBe("Recent places");
    const places = (): (string | null)[] =>
      Array.from(target.querySelectorAll("[role=option]")).map((r) => r.getAttribute("title"));
    expect(places()).toEqual([
      "docs/guide.md#setup",
      "notes/ideas.md",
      "docs/guide.md",
      "README.md",
    ]);
    expect(target.querySelector("[role=option]")?.textContent).toContain("You are here");
    expect(rowTexts().filter((text) => text.includes("You are here"))).toHaveLength(1);
    const input = target.querySelector<HTMLInputElement>("input.field");
    if (!input) throw new Error("no field");
    expect(input.placeholder).toBe("Filter recent places...");
    type(input, "read");
    flushSync();
    await tick();
    expect(places()).toEqual(["README.md"]);
    const jump = vi.spyOn(documents, "jump").mockResolvedValue();
    press(input, "Enter");
    expect(jump).toHaveBeenCalledWith(0);
    expect(ui.finder.status).toBe("closed");
    jump.mockRestore();
  });

  it("is not offered while there is nowhere else to go", () => {
    documents.trail.clear();
    documents.trail.visit({ path: "README.md", anchor: null });
    const command = contributions.of("command").find((c) => c.id === "trail.show");
    expect(command?.when?.()).toBe(false);
  });
});
