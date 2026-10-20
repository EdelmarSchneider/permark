// @vitest-environment jsdom
// The shell's own screens against the mock backend: the settings page (sections, plugins, project settings), the
// initialize question, the top bar (chips, Open menu, closing) and its keys, the welcome screen.
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
// jsdom has no layout: the chip row's size watcher sees nothing (the real web views have one)
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};

const { api } = await import("../../api");
const { default: SettingsPage } = await import("./settings/SettingsPage.svelte");
const { default: InitDialog } = await import("./InitDialog.svelte");
const { default: TopBar } = await import("./TopBar.svelte");
const { closeHere } = await import("./closeWorkspace");
const { default: CloseWindowDialog } = await import("./CloseWindowDialog.svelte");
const { default: Welcome } = await import("./Welcome.svelte");
const { ui } = await import("./ui.svelte");
const { windowStore } = await import("../window.svelte");
const { tabs } = await import("./tabs.svelte");
const { registerCoreCommands } = await import("../commands/core");
const { contributions } = await import("../plugins/instance.svelte");
const { conditions } = await import("../conditions/context.svelte");
const { pluginsStore, projectSettings } = await import("./settings/stores.svelte");
const { confirmInit, askInitHere } = await import("./initProject");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const store = windowStore;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let unregister: (() => void) | null = null;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 10 });
}

async function open(): Promise<void> {
  if (!started) {
    await store.start();
    started = true;
  }
  await mock.openPath(MOCK_FOLDER, "thisTab");
  await until(
    () => store.view.state === "showing" && store.view.workspace.kind === "project",
    "the project shows",
  );
}

/** Every tab of the mock's window closed: the Welcome. */
async function toWelcome(): Promise<void> {
  const listed = await mock.listWindows();
  for (const tab of listed.ok ? (listed.value[0]?.tabs ?? []) : []) await mock.tabClose(tab.tab);
}

function press(el: Element | Window, key: string, init: KeyboardEventInit = {}): void {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
}

function byText<T extends HTMLElement = HTMLElement>(selector: string, text: string): T {
  const found = Array.from(target.querySelectorAll<T>(selector)).find((el) =>
    el.textContent?.replace(/\s+/g, " ").trim().includes(text),
  );
  if (!found) throw new Error(`no ${selector} with "${text}"`);
  return found;
}

function mountPage(section: "reading" | "editing" | "integration" | "plugins" | "project"): void {
  ui.openSettingsPage(section);
  component = mount(SettingsPage, { target, props: { store } });
  flushSync();
}

beforeEach(async () => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  ui.closeSettingsPage();
  ui.closeInit();
  unregister = registerCoreCommands(contributions);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  unregister?.();
  // put the shared mock back where the next test expects it
  await store.changeSetting({ key: "openIn", value: "tab" });
  for (const id of ["git", "unread", "review", "notes", "agent", "updates", "shell-integration"]) {
    await store.changeSetting({ key: "pluginEnabled", value: { plugin: id, enabled: null } });
  }
  mock.probe.shell.setOthers([]);
  mock.probe.shell.forgetInit();
  mock.probe.shell.removed.length = 0;
  mock.probe.shell.focused.length = 0;
  mock.probe.tabs.moved.length = 0;
  await toWelcome();
});

describe("the settings page", () => {
  it("lists Reading, Editing, Integration, Plugins - Project only while a project is shown", async () => {
    await store.start().catch(() => {});
    started = true;
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    mountPage("reading");
    const names = () =>
      Array.from(target.querySelectorAll(".nav-item")).map((b) => b.textContent?.trim());
    expect(names()).toEqual(["Reading", "Editing", "Integration", "Plugins"]);
    // width and zoom have no page to act on yet, and say so
    expect(target.textContent).toContain("Open a document to change the width and the zoom.");
    await unmount(component!);
    component = null;
    await open();
    mountPage("reading");
    await tick();
    expect(names()).toEqual(["Reading", "Editing", "Integration", "Plugins", "Project"]);
  });

  it("Escape closes it and a section the person cannot reach falls back to Reading", async () => {
    await open();
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    mountPage("project");
    expect(target.querySelector("h3.title")?.textContent).toBe("Reading");
    const page = target.querySelector<HTMLElement>(".page");
    press(page!, "Escape");
    expect(ui.settingsPage.status).toBe("closed");
  });

  it("the section list is a keyboard list: arrows move the section and the focus", async () => {
    await open();
    mountPage("reading");
    const nav = target.querySelector<HTMLElement>('[aria-current="page"]')!;
    nav.focus();
    press(nav, "ArrowDown");
    await tick();
    expect(target.querySelector("h3.title")?.textContent).toBe("Editing");
    expect(document.activeElement?.textContent?.trim()).toBe("Editing");
    press(document.activeElement!, "ArrowUp");
    press(document.activeElement!, "ArrowUp");
    await tick();
    expect(target.querySelector("h3.title")?.textContent).toBe("Project");
  });

  it("Editing: a refused editor command is said in words, a good one is kept with its preview", async () => {
    await open();
    mountPage("editing");
    const select = target.querySelector<HTMLSelectElement>("#set-editor")!;
    select.value = "custom";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await tick();
    const refuse = vi.spyOn(mock, "settingsSet").mockResolvedValueOnce({
      ok: false,
      error: {
        kind: "backend",
        command: "settings_set",
        error: { kind: "invalidData", subject: "editor", detail: "unbalancedQuote" },
      },
    });
    const field = target.querySelector<HTMLInputElement>('input[aria-label="Command"]')!;
    field.value = 'code "-g {path}';
    field.dispatchEvent(new Event("change", { bubbles: true }));
    await until(
      () => target.textContent?.includes("A quote is not closed.") === true,
      "the reason",
    );
    refuse.mockRestore();
    field.value = "code -g {path}:{line}:{col}";
    field.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => store.editor === "code -g {path}:{line}:{col}", "kept");
    await until(
      () => target.textContent?.includes("code -g /docs/plan.md:12:1") === true,
      "the preview",
    );
    await store.changeSetting({ key: "editor", value: "" });
  });

  it("Editing: where folders and files open is a setting (New tab | This tab | New window)", async () => {
    await open();
    mountPage("editing");
    const options = Array.from(target.querySelectorAll("button[role=radio]")).map((b) =>
      b.textContent?.trim(),
    );
    expect(options).toEqual(expect.arrayContaining(["New tab", "This tab", "New window"]));
    expect(target.textContent).toContain("Already open: PerMark shows it where it is.");
    byText("button[role=radio]", "New window").click();
    await until(() => store.openIn === "window", "kept");
    expect(
      target.querySelector('button[role=radio][aria-checked="true"]:not(:disabled)')?.textContent,
    ).toBeTruthy();
  });

  it("Integration: the default app, then the rows of the plugins that are on", async () => {
    await open();
    await until(
      () => contributions.visible("settingsSection", conditions.current).length >= 4,
      `plugins ${contributions
        .visible("settingsSection", conditions.current)
        .map((c) => c.id)
        .join(",")} / ${conditions.current.plugins.join(",")}`,
    );
    mountPage("integration");
    await until(() => target.querySelectorAll("section.group").length >= 5, "groups");
    const headings = Array.from(target.querySelectorAll("section.group h3")).map((h) =>
      h.textContent?.trim(),
    );
    expect(headings[0]).toBe("Default app");
    expect(headings).toEqual(expect.arrayContaining(["File manager", "Updates", "Git", "Notes"]));
    // Windows: who opens the files now, and the way to Windows' own page
    await until(
      () => target.textContent?.includes("Choose in Windows Settings") === true,
      "handoff",
    );
    expect(target.textContent).toContain("Opens with PerMark");
    // plugins off: their sections go, the default app stays
    for (const id of ["git", "notes", "agent", "updates", "shell-integration"]) {
      await store.changeSetting({ key: "pluginEnabled", value: { plugin: id, enabled: false } });
    }
    await until(
      () => target.querySelectorAll("section.group").length === 1,
      "only the default app",
    );
    expect(target.textContent?.toLowerCase()).not.toContain("update");
  });
});

describe("the plugins page", () => {
  async function pluginsPage(): Promise<void> {
    await open();
    mountPage("plugins");
    await until(() => pluginsStore.state.status === "ready", "the list");
    await tick();
  }
  const row = (name: string): HTMLElement =>
    byText(".plugin", name).closest<HTMLElement>(".plugin") as HTMLElement;
  const statusOf = (name: string): string =>
    row(name).querySelector(".status")?.textContent?.replace(/\s+/g, " ").trim() ?? "";
  const switchOf = (name: string): HTMLInputElement =>
    row(name).querySelector<HTMLInputElement>('input[role="switch"]') as HTMLInputElement;

  it("every plugin is listed once, grouped by kind, with its switch on", async () => {
    await pluginsPage();
    expect(target.querySelectorAll(".plugin")).toHaveLength(34);
    const groups = Array.from(target.querySelectorAll("section.group h3")).map(
      (h) => h.textContent,
    );
    expect(groups).toEqual([
      "Features",
      "File formats",
      "File roles",
      "Analyses",
      "Checkers",
      "Blocks in Markdown",
    ]);
    // on by default, except the specialist tools that start off
    const off = Array.from(target.querySelectorAll<HTMLInputElement>('input[role="switch"]'))
      .filter((i) => !i.checked)
      .map((i) => i.closest(".plugin")?.querySelector(".name")?.textContent?.trim());
    expect(off).toEqual(["Converter", "Token count"]);
  });

  it("turning a plugin off turns its dependents off with a reason and a way back", async () => {
    await pluginsPage();
    switchOf("Unread markers").click();
    await until(
      () => /Needs Unread markers/.test(row("Review").textContent ?? ""),
      "dependent says why",
    );
    expect(switchOf("Unread markers").checked).toBe(false);
    // the dependent's own switch is still the person's choice (on), only the state is off
    expect(switchOf("Review").checked).toBe(true);
    byText("button.link", "Turn on Unread markers").click();
    await until(() => statusOf("Review") === "", "back");
    expect(switchOf("Unread markers").checked).toBe(true);
  });

  it("data can be removed only from a plugin that is off, after a question", async () => {
    await pluginsPage();
    expect(row("Comments").textContent).not.toContain("Remove data");
    switchOf("Tags and descriptions").click();
    await until(
      () => row("Tags and descriptions").textContent?.includes("Remove data") === true,
      "off, with the way to remove its data",
    );
    byText("button", "Remove data").click();
    await tick();
    expect(target.textContent).toContain("Remove the data of Tags and descriptions?");
    expect(mock.probe.shell.removed).toEqual([]);
    byText("button", "Cancel").click();
    await tick();
    expect(mock.probe.shell.removed).toEqual([]);
    byText("button", "Remove data").click();
    await tick();
    byText("button", "Remove").click();
    await until(() => mock.probe.shell.removed.length === 1, "removed");
    expect(mock.probe.shell.removed).toEqual(["tags"]);
    expect(store.notices.at(-1)).toMatchObject({
      kind: "note",
      text: "Removed the data of Tags and descriptions.",
    });
    await store.changeSetting({ key: "pluginEnabled", value: { plugin: "tags", enabled: null } });
  });

  it("a plugin of the project can be turned off for this project alone", async () => {
    await pluginsPage();
    const select = row("Comments").querySelector<HTMLSelectElement>("select")!;
    expect(select.value).toBe("app");
    select.value = "off";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => /Off in this project/.test(statusOf("Comments")), "off here");
    // the app's switch is untouched; the plugin is not offered for data removal
    expect(switchOf("Comments").checked).toBe(true);
    expect(row("Comments").textContent).not.toContain("Remove data");
    row("Comments").querySelector<HTMLSelectElement>("select")!.value = "app";
    row("Comments")
      .querySelector("select")!
      .dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => statusOf("Comments") === "", "follows the app");
  });

  it("a plugin of the app has no project choice, and a window with no project says what project plugins need", async () => {
    await open();
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    mountPage("plugins");
    await until(() => pluginsStore.state.status === "ready", "the list");
    await tick();
    expect(row("Notes").querySelector("select")).toBeNull();
    expect(row("Git").textContent).toContain("Open a folder first.");
    expect(row("Git").querySelector("select")).toBeNull();
    expect(row("Mermaid diagrams").textContent).not.toContain("Open a folder");
  });
});

describe("the project settings and Initialize PerMark", () => {
  it("not initialized: the section only offers to create .permark; yes creates it and the controls appear", async () => {
    await open();
    mountPage("project");
    await tick();
    expect(target.textContent).toContain("PerMark is not initialized in this project");
    expect(target.querySelector(".list")).toBeNull();
    byText("button", "Initialize PerMark").click();
    expect(ui.initDialog.status).toBe("asking");
    await confirmInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await until(() => target.querySelector(".list") !== null, "the controls");
    expect(ui.initDialog.status).toBe("closed");
  });

  it("patterns are added and removed one by one, kept at once; a refused value says why", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    mountPage("project");
    await until(() => projectSettings.state.status === "ready", "loaded");
    await tick();
    const input = target.querySelector<HTMLInputElement>('input[aria-label="Ignore patterns"]')!;
    input.value = "drafts/";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    press(input, "Enter");
    await until(() => mock.probe.shell.project().ignore.includes("drafts/"), "kept");
    await until(() => target.querySelector(".list li code")?.textContent === "drafts/", "listed");
    // a line break never reaches the file
    input.value = "a\u0001b";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    press(input, "Enter");
    await until(
      () => target.textContent?.includes("Line breaks and control characters") === true,
      "reason",
    );
    expect(mock.probe.shell.project().ignore).toEqual(["drafts/"]);
    target.querySelector<HTMLElement>('button[aria-label="Remove drafts/"]')!.click();
    await until(() => mock.probe.shell.project().ignore.length === 0, "removed");
    // the preset
    byText("button[role=radio]", "Everything").click();
    await until(() => mock.probe.shell.project().tree.show === "everything", "preset kept");
    byText("button[role=radio]", "Not set").click();
    await until(() => mock.probe.shell.project().tree.show === null, "preset back");
  });

  it("the agent's narrowing is part of the section while the agent plugin is on", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await until(
      () => contributions.visible("projectSection", conditions.current).length === 1,
      "the agent part",
    );
    mountPage("project");
    await until(() => projectSettings.state.status === "ready", "loaded");
    await until(
      () => target.textContent?.includes("Groups agents never get here") === true,
      "shown",
    );
    const box = byText("label.group", "Read notes you share").querySelector<HTMLInputElement>(
      "input",
    )!;
    box.click();
    await until(() => mock.probe.shell.project().agentDeny.includes("notes"), "narrowed");
    await store.changeSetting({ key: "pluginEnabled", value: { plugin: "agent", enabled: false } });
    await until(
      () => contributions.visible("projectSection", conditions.current).length === 0,
      "gone with the plugin",
    );
  });

  it("the question names the folder and what is created; Escape and Cancel change nothing", async () => {
    await open();
    askInitHere();
    expect(ui.initDialog).toMatchObject({ status: "asking", path: MOCK_FOLDER });
    const answers: boolean[] = [];
    component = mount(InitDialog, {
      target,
      props: {
        name: "permark-docs",
        path: MOCK_FOLDER,
        answer: (yes: boolean) => answers.push(yes),
      },
    });
    flushSync();
    expect(target.textContent).toContain("Initialize PerMark in permark-docs?");
    expect(target.textContent).toContain(".permark/config.toml");
    expect(target.textContent).toContain("Your files are not touched.");
    press(target.querySelector(".dialog")!, "Escape");
    byText("button", "Cancel").click();
    byText("button.btn--primary", "Initialize PerMark").click();
    expect(answers).toEqual([false, false, true]);
    ui.closeInit();
  });

  it("the command and the remedy exist only while the project is not initialized", async () => {
    await open();
    const initialized = conditions.current.permark === "initialized";
    const command = contributions.of("command").find((c) => c.id === "project.init")!;
    expect(command.when?.()).toBe(!initialized);
  });
});

describe("the top bar", () => {
  const FILE = "C:/p/proposal-for-the-quarterly-budget-review.md";

  /** This window: the project, api-specs, a long file, the Notes; the project active. */
  async function fourTabs(): Promise<number[]> {
    await open();
    await mock.openPath("C:/work/api-specs", "tab");
    await mock.openPath(FILE, "tab");
    await mock.openNotes("tab");
    const listed = await mock.listWindows();
    const ids = listed.ok ? (listed.value[0]?.tabs.map((t) => t.tab) ?? []) : [];
    await mock.tabActivate(ids[0] ?? 0);
    return ids;
  }

  const chips = (): HTMLElement[] => Array.from(target.querySelectorAll<HTMLElement>("[data-tab]"));
  const closeOf = (at: number): HTMLElement =>
    target.querySelectorAll<HTMLElement>(".chip")[at]!.querySelector<HTMLElement>(".close")!;
  const shownName = (): string | null =>
    store.view.state === "showing" ? store.view.workspace.name : null;

  async function mountBar(notes = true): Promise<{ ids: number[]; stop: () => void }> {
    const ids = await fourTabs();
    const stop = tabs.start("main", mock);
    await until(() => tabs.list.length === 4, "the tabs");
    component = mount(TopBar, { target, props: { store, notes } });
    flushSync();
    return { ids, stop };
  }

  it("one chip per tab of this window, also with one; another window's tabs never show", async () => {
    await open();
    const stop = tabs.start("main", mock);
    await until(() => tabs.list.length === 1, "one tab");
    expect(tabs.canSwitch).toBe(false);
    component = mount(TopBar, { target, props: { store, notes: true } });
    flushSync();
    expect(chips().map((c) => c.getAttribute("aria-label"))).toEqual(["permark-docs"]);
    expect(closeOf(0).getAttribute("aria-label")).toBe("Close permark-docs (Ctrl+W)");
    await unmount(component);
    component = null;
    stop();
    const bar = await mountBar();
    mock.probe.shell.setOthers([
      {
        window: "w-2",
        kind: "work",
        tabs: [
          {
            tab: 77,
            kind: "project",
            title: "elsewhere",
            path: "C:/elsewhere",
            loaded: true,
            unsaved: false,
          },
        ],
        active: 77,
        focusedMs: 0,
      },
    ]);
    await tick();
    expect(tabs.canSwitch).toBe(true);
    expect(chips().map((c) => c.getAttribute("aria-label"))).toEqual([
      "permark-docs",
      "api-specs",
      "proposal-for-the-quarterly-budget-review.md",
      "Notes",
    ]);
    expect(chips()[0]?.getAttribute("aria-current")).toBe("page");
    expect(chips()[1]?.getAttribute("aria-current")).toBeNull();
    expect(chips()[1]?.getAttribute("title")).toBe("C:/work/api-specs");
    // a long name is cut in the middle: the end of the name (and its extension) always shows
    expect(chips()[2]?.querySelector(".tail")?.textContent).toBe("view.md");
    // only the active chip names the shortcut
    expect(closeOf(1).getAttribute("aria-label")).toBe("Close api-specs");
    // each chip wears its kind's hue (Notes yellow, a file gray)
    const hue = (at: number): string =>
      target.querySelectorAll<HTMLElement>(".chip")[at]!.style.getPropertyValue("--hue");
    expect([hue(2), hue(3)]).toEqual(["var(--tag-gray)", "var(--tag-yellow)"]);
    // no window count: a workspace lives in one place
    expect(target.querySelector(".count")).toBeNull();
    bar.stop();
  });

  it("the unsaved dot follows the tab's unsaved edits both ways", async () => {
    const bar = await mountBar();
    expect(target.querySelector("[data-unsaved]")).toBeNull();
    await mock.editSetUnsaved("guide.md");
    await mock.tabActivate(bar.ids[1]!);
    await mock.tabActivate(bar.ids[0]!);
    await until(() => target.querySelector("[data-unsaved]") !== null, "the dot");
    expect(chips()[0]?.getAttribute("aria-label")).toBe("permark-docs, Unsaved changes");
    await mock.editSetUnsaved(null);
    await mock.tabActivate(bar.ids[1]!);
    await until(() => target.querySelector("[data-unsaved]") === null, "no dot");
    bar.stop();
  });

  it("a click shows that tab in this window", async () => {
    const bar = await mountBar();
    chips()[1]!.click();
    await until(() => shownName() === "api-specs", "api-specs shows");
    expect(tabs.active).toBe(bar.ids[1]);
    expect(chips()[1]?.getAttribute("aria-current")).toBe("page");
    bar.stop();
  });

  it("closes from the close button, the Delete key or a middle click", async () => {
    const bar = await mountBar();
    closeOf(1).click();
    await until(() => chips().length === 3, "three");
    press(chips()[1]!, "Delete");
    await until(() => chips().length === 2, "two");
    chips()[1]!.dispatchEvent(new MouseEvent("auxclick", { bubbles: true, button: 1 }));
    await until(() => chips().length === 1, "one");
    // other mouse buttons and Delete with a modifier do nothing
    chips()[0]!.dispatchEvent(new MouseEvent("auxclick", { bubbles: true, button: 2 }));
    press(chips()[0]!, "Delete", { ctrlKey: true });
    await tick();
    expect(chips()).toHaveLength(1);
    expect(shownName()).toBe("permark-docs");
    bar.stop();
  });

  it("the last tab closed: the window shows the Welcome and the bar the app's name", async () => {
    await open();
    const stop = tabs.start("main", mock);
    await until(() => tabs.list.length === 1, "one tab");
    component = mount(TopBar, { target, props: { store, notes: true } });
    flushSync();
    closeOf(0).click();
    await until(() => store.view.state === "welcome", "welcome");
    await tick();
    expect(target.querySelector(".brand")?.textContent).toBe("PerMark");
    stop();
  });

  it("the arrows move between chips (wrapping), Home and End go to the ends", async () => {
    const bar = await mountBar();
    const all = chips();
    all[0]?.focus();
    press(all[0]!, "ArrowLeft");
    expect(document.activeElement).toBe(all[3]);
    press(all[3]!, "ArrowRight");
    expect(document.activeElement).toBe(all[0]);
    press(all[0]!, "End");
    expect(document.activeElement).toBe(all[3]);
    press(all[3]!, "Home");
    expect(document.activeElement).toBe(all[0]);
    // only one chip is in the tab order (the active one); the close buttons are reached by Delete | the menu
    expect(all.map((t) => t.tabIndex)).toEqual([0, -1, -1, -1]);
    const closes = Array.from(target.querySelectorAll<HTMLElement>(".close"));
    expect(closes.map((b) => b.tabIndex)).toEqual([-1, -1, -1, -1]);
    bar.stop();
  });

  it("the context menu moves a tab to a new window or closes it; one tab cannot move", async () => {
    const bar = await mountBar();
    const chip = chips()[1]!;
    chip.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 40, clientY: 30 }),
    );
    await tick();
    const items = Array.from(document.querySelectorAll('[role="menuitem"]')).map((i) =>
      i.textContent?.trim(),
    );
    expect(items).toEqual(["Move to new window", "Close"]);
    (document.querySelectorAll('[role="menuitem"]')[0] as HTMLElement).click();
    await until(() => chips().length === 3, "moved out");
    expect(mock.probe.tabs.moved).toEqual([bar.ids[1]]);
    // the keyboard way in: the Menu key
    press(chips()[0]!, "ContextMenu");
    await tick();
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe("Tab actions");
    press(document.querySelector('[role="menu"]')!, "Escape");
    await tick();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    // a single tab: only Close
    await mock.tabClose(bar.ids[2]!);
    await mock.tabClose(bar.ids[3]!);
    await until(() => chips().length === 1, "one");
    chips()[0]!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await tick();
    const single = Array.from(document.querySelectorAll('[role="menuitem"]')).map((i) =>
      i.textContent?.trim(),
    );
    expect(single).toEqual(["Close Ctrl+W"]);
    bar.stop();
  });

  it("the Open menu: folder, file, new project, Notes, then the recent folders that exist", async () => {
    const bar = await mountBar();
    await mock.tabActivate(bar.ids[1]!);
    const button = target.querySelector<HTMLElement>("button.open")!;
    expect(button.getAttribute("aria-label")).toBe("Open");
    const menuShows = () => until(() => document.querySelector('[role="menu"]') !== null, "menu");
    button.click();
    await menuShows();
    const menu = document.querySelector('[role="menu"]')!;
    expect(menu.getAttribute("aria-label")).toBe("Open");
    const items = Array.from(menu.querySelectorAll('[role="menuitem"]')).map((i) =>
      i.textContent?.replace(/\s+/g, " ").trim(),
    );
    // the missing recent folder is left out (the Welcome lists it with its Remove)
    expect(items).toEqual([
      "Open folder... Ctrl+O",
      "Open file... Ctrl+Shift+O",
      "New project…",
      "Notes",
      "api-specs C:/work/api-specs",
      `permark-docs ${MOCK_FOLDER}`,
    ]);
    expect(menu.querySelector(".heading")?.textContent).toBe("Recent folders");
    // a recent folder that is open already: its tab comes forward, nothing opens again
    (menu.querySelectorAll('[role="menuitem"]')[5] as HTMLElement).click();
    await until(() => shownName() === "permark-docs", "its tab");
    expect(tabs.list).toHaveLength(4);
    // the Notes are open: the same
    button.click();
    await menuShows();
    (document.querySelectorAll('[role="menuitem"]')[3] as HTMLElement).click();
    await until(
      () => store.view.state === "showing" && store.view.workspace.kind === "notes",
      "Notes",
    );
    expect(tabs.list).toHaveLength(4);
    // a second click on the button closes the menu it opened
    button.click();
    await menuShows();
    button.click();
    await tick();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    bar.stop();
  });

  it("the Open menu leaves the Notes out while their plugin is off", async () => {
    const bar = await mountBar(false);
    target.querySelector<HTMLElement>("button.open")!.click();
    await until(() => document.querySelector('[role="menu"]') !== null, "menu");
    const items = Array.from(document.querySelectorAll('[role="menuitem"]')).map((i) =>
      i.textContent?.trim(),
    );
    expect(items.some((i) => i === "Notes")).toBe(false);
    bar.stop();
  });

  it("with no tab in the window: the app name, no chips, no Open button; settings always there", async () => {
    await store.start().catch(() => {});
    started = true;
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    const stop = tabs.start("main", mock);
    await until(() => tabs.list.length === 0, "nothing open");
    component = mount(TopBar, { target, props: { store, notes: true } });
    flushSync();
    expect(target.querySelector(".brand")?.textContent).toBe("PerMark");
    expect(chips()).toHaveLength(0);
    expect(target.querySelector("button.open")).toBeNull();
    const settings = target.querySelector<HTMLElement>("[data-settings-trigger]")!;
    expect(settings.getAttribute("aria-label")).toBe("Settings");
    settings.click();
    expect(ui.settings).toBe(true);
    ui.closeSettingsPopover();
    stop();
  });

  it("tab keys: next | previous wrap, Mod+1..8 tab n, Mod+9 the last, Mod+W closes the active one", async () => {
    const bar = await mountBar();
    const command = (id: string) => contributions.of("command").find((c) => c.id === id)!;
    const shown = (): number | null => tabs.active;
    await command("tab.previous").run();
    await until(() => shown() === bar.ids[3], "wrapped to the last");
    await command("tab.next").run();
    await until(() => shown() === bar.ids[0], "wrapped to the first");
    await command("tab.select3").run();
    await until(() => shown() === bar.ids[2], "third");
    await command("tab.selectLast").run();
    await until(() => shown() === bar.ids[3], "last");
    // tab 8 does not exist: nothing
    expect(command("tab.select8").when?.()).toBe(false);
    await command("tab.close").run();
    await until(() => tabs.list.length === 3, "closed");
    expect(tabs.list.map((t) => t.tab)).toEqual(bar.ids.slice(0, 3));
    const keys = contributions
      .of("shortcut")
      .filter((s) => s.command.startsWith("tab."))
      .map((k) => [k.command, k.keys]);
    expect(keys).toEqual([
      ["tab.next", "Ctrl+Tab"],
      ["tab.next", "Mod+Alt+ArrowRight"],
      ["tab.previous", "Ctrl+Shift+Tab"],
      ["tab.previous", "Mod+Alt+ArrowLeft"],
      ...[1, 2, 3, 4, 5, 6, 7, 8].map((n) => [`tab.select${n}`, `Mod+${n}`]),
      ["tab.selectLast", "Mod+9"],
      ["tab.close", "Mod+w"],
    ]);
    bar.stop();
  });

  it("with one tab the switching keys do nothing", async () => {
    await open();
    const stop = tabs.start("main", mock);
    await until(() => tabs.list.length === 1, "one tab");
    const command = (id: string) => contributions.of("command").find((c) => c.id === id)!;
    expect(command("tab.next").when?.()).toBe(false);
    expect(command("tab.previous").when?.()).toBe(false);
    stop();
  });
});

describe("closing a tab", () => {
  it("its unsaved edits are settled first, then the backend closes it", async () => {
    const calls: string[] = [];
    const fake = {
      tabCloseConfirmed: async () => {
        calls.push("confirmed");
        return { ok: true as const, value: null };
      },
    };
    await closeHere({ api: fake } as unknown as NonNullable<Parameters<typeof closeHere>[0]>);
    expect(calls).toEqual(["confirmed"]);
  });

  it("unsaved edits the person keeps (Cancel): nothing closes", async () => {
    const { setLeaveGuard } = await import("../editing/guard");
    const off = setLeaveGuard(async () => false);
    const calls: string[] = [];
    const fake = {
      tabCloseConfirmed: async () => {
        calls.push("confirmed");
        return { ok: true as const, value: null };
      },
    };
    await closeHere({ api: fake } as unknown as NonNullable<Parameters<typeof closeHere>[0]>);
    expect(calls).toEqual([]);
    off();
  });
});

describe("the window close prompt", () => {
  it("lists every tab with unsaved edits and answers Save all | Discard | Cancel", async () => {
    const answers: string[] = [];
    component = mount(CloseWindowDialog, {
      target,
      props: {
        tabs: [
          { tab: 1, title: "permark-docs", document: "docs/guide.md" },
          { tab: 2, title: "todo.md", document: "C:/x/todo.md" },
        ],
        answer: (a: string) => answers.push(a),
      },
    });
    flushSync();
    const dialog = target.querySelector<HTMLElement>("[data-close-window]")!;
    expect(dialog.getAttribute("role")).toBe("alertdialog");
    expect(target.textContent).toContain("Save changes before closing?");
    expect(target.textContent).toContain("These tabs have unsaved changes:");
    const rows = Array.from(target.querySelectorAll(".tabs li")).map((li) =>
      li.textContent?.replace(/\s+/g, " ").trim(),
    );
    // a single file names itself once
    expect(rows).toEqual(["permark-docs guide.md", "todo.md"]);
    expect(document.activeElement?.textContent?.trim()).toBe("Save all");
    byText("button", "Discard").click();
    byText("button", "Save all").click();
    byText("button", "Cancel").click();
    press(dialog, "Escape");
    expect(answers).toEqual(["discard", "saveAll", "cancel", "cancel"]);
  });

  it("one tab: the same question, in the singular", async () => {
    component = mount(CloseWindowDialog, {
      target,
      props: { tabs: [{ tab: 1, title: "a", document: "a.md" }], answer: () => {} },
    });
    flushSync();
    expect(target.textContent).toContain("This tab has unsaved changes:");
  });

  it("the mock backend: one prompt when the window holds unsaved edits, none otherwise", async () => {
    await open();
    const asked: number[] = [];
    const off = await mock.on("close-requested", (e) => asked.push(e.tabs.length));
    mock.probe.edit.closed = false;
    await mock.windowClose();
    expect(mock.probe.edit.closed).toBe(true);
    mock.probe.edit.closed = false;
    await mock.editSetUnsaved("guide.md");
    await mock.windowClose();
    expect(asked).toEqual([1]);
    expect(mock.probe.edit.closed).toBe(false);
    await mock.editSetUnsaved(null);
    off();
  });
});

describe("the welcome screen", () => {
  it("offers Open folder, Open file and New project, lists the recent folders with their age", async () => {
    await store.start().catch(() => {});
    started = true;
    await toWelcome();
    await until(() => store.view.state === "welcome" && store.recent.length > 0, "welcome");
    component = mount(Welcome, { target, props: { store } });
    flushSync();
    const buttons = Array.from(target.querySelectorAll(".actions .btn")).map((b) =>
      b.textContent?.replace(/\s+/g, " ").trim(),
    );
    expect(buttons[0]).toContain("Open folder");
    expect(buttons[1]).toContain("Open file");
    expect(buttons[2]).toBe("New project…");
    const rows = Array.from(target.querySelectorAll('[role="option"]'));
    const docs = rows.find((r) => r.textContent?.includes(MOCK_FOLDER));
    expect(docs?.querySelector(".age")?.textContent).toMatch(/now|hour/);
    // a folder that is gone is marked and removable, and does not open
    const gone = rows.find((r) => r.textContent?.includes("(not found)"));
    expect(gone?.querySelector("button")?.textContent).toBe("Remove");
    expect(target.textContent).toContain("Drop a folder or a .md file anywhere to open it.");
  });

  it("New project picks a folder, initializes it and opens it as a project", async () => {
    await store.start().catch(() => {});
    started = true;
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    component = mount(Welcome, { target, props: { store } });
    flushSync();
    byText("button", "New project").click();
    await until(() => store.view.state === "showing", "opened");
    expect(mock.probe.shell.project().initialized).toBe(true);
    await until(() => conditions.current.permark === "initialized", "initialized");
  });
});
