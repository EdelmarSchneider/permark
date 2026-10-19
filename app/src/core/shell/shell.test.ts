// @vitest-environment jsdom
// The shell's own screens against the mock backend: the settings page (sections, plugins, project settings), the
// initialize question, the switcher bar and its keys, the welcome screen.
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

const { api } = await import("../../api");
const { default: SettingsPage } = await import("./settings/SettingsPage.svelte");
const { default: InitDialog } = await import("./InitDialog.svelte");
const { default: Switcher } = await import("./Switcher.svelte");
const { default: Welcome } = await import("./Welcome.svelte");
const { ui } = await import("./ui.svelte");
const { windowStore } = await import("../window.svelte");
const { switcher } = await import("./workspaceList.svelte");
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
  await mock.openPath(MOCK_FOLDER, "thisWindow");
  await until(() => store.view.state === "showing", "the project shows");
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
  await store.changeSetting({ key: "openIn", value: "ask" });
  for (const id of ["git", "unread", "review", "notes", "agent", "updates", "shell-integration"]) {
    await store.changeSetting({ key: "pluginEnabled", value: { plugin: id, enabled: null } });
  }
  mock.probe.shell.setOthers([]);
  mock.probe.shell.forgetInit();
  mock.probe.shell.removed.length = 0;
  mock.probe.shell.focused.length = 0;
  mock.probe.shell.closed.length = 0;
});

describe("the settings page", () => {
  it("lists Reading, Editing, Integration, Plugins - Project only while a project is shown", async () => {
    await store.start().catch(() => {});
    started = true;
    await mock.showWelcome();
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
    await mock.showWelcome();
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

  it("Editing: the open-over choice is a setting", async () => {
    await open();
    mountPage("editing");
    byText("button[role=radio]", "New window").click();
    await until(() => store.openIn === "new", "kept");
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
    expect(target.querySelectorAll(".plugin")).toHaveLength(31);
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
    expect(
      Array.from(target.querySelectorAll<HTMLInputElement>('input[role="switch"]')).every(
        (i) => i.checked,
      ),
    ).toBe(true);
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
    await mock.showWelcome();
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

describe("the switcher bar", () => {
  const list = (): void =>
    mock.probe.shell.setOthers([
      { id: 90, kind: "project", title: "api-specs", path: "C:/work/api-specs", windows: 2 },
      { id: 91, kind: "singleFile", title: "proposal.md", path: "C:/p/proposal.md", windows: 1 },
      { id: 92, kind: "notes", title: "Notes", path: "C:/Notes", windows: 1 },
    ]);

  async function mountBar(): Promise<void> {
    await open();
    switcher.start(mock);
    list();
    await until(() => switcher.list.length === 4, "the list");
    component = mount(Switcher, { target, props: { current: switcher.list[0]?.id ?? null } });
    flushSync();
  }

  it("is for two or more: one tab per workspace, its window count when it has several", async () => {
    await open();
    const stop = switcher.start(mock);
    await until(() => switcher.list.length === 1, "one workspace");
    expect(switcher.visible).toBe(false);
    list();
    await until(() => switcher.visible, "now it shows");
    stop();
    await mountBar();
    const tabs = Array.from(target.querySelectorAll("[data-tab]"));
    expect(tabs.map((t) => t.textContent?.replace(/\s+/g, " ").trim())).toEqual([
      "permark-docs",
      "api-specs 2 2 windows",
      "proposal.md",
      "Notes",
    ]);
    expect(tabs[0]?.getAttribute("aria-current")).toBe("true");
    expect(tabs[1]?.getAttribute("title")).toBe("C:/work/api-specs");
  });

  it("a click focuses the window that shows the workspace", async () => {
    await mountBar();
    (target.querySelectorAll("[data-tab]")[2] as HTMLElement).click();
    expect(mock.probe.shell.focused).toEqual([91]);
  });

  it("the arrows move between tabs (wrapping), Home and End go to the ends", async () => {
    await mountBar();
    const tabs = Array.from(target.querySelectorAll<HTMLElement>("[data-tab]"));
    tabs[0]?.focus();
    press(tabs[0]!, "ArrowLeft");
    expect(document.activeElement).toBe(tabs[3]);
    press(tabs[3]!, "ArrowRight");
    expect(document.activeElement).toBe(tabs[0]);
    press(tabs[0]!, "End");
    expect(document.activeElement).toBe(tabs[3]);
    press(tabs[3]!, "Home");
    expect(document.activeElement).toBe(tabs[0]);
    // only one tab is in the tab order: the one this window shows
    expect(tabs.map((t) => t.tabIndex)).toEqual([0, -1, -1, -1]);
  });

  it("the context menu opens in a new window or closes the workspace", async () => {
    await mountBar();
    const tab = target.querySelectorAll<HTMLElement>("[data-tab]")[1]!;
    tab.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 40, clientY: 30 }),
    );
    await tick();
    const items = Array.from(document.querySelectorAll('[role="menuitem"]')).map((i) =>
      i.textContent?.trim(),
    );
    expect(items).toEqual(["Open in a new window", "Close"]);
    (document.querySelectorAll('[role="menuitem"]')[1] as HTMLElement).click();
    expect(mock.probe.shell.closed).toEqual([90]);
    // the keyboard way in: the Menu key
    press(tab, "ContextMenu");
    await tick();
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "Workspace actions",
    );
    press(document.querySelector('[role="menu"]')!, "Escape");
    await tick();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("Ctrl+Alt+Right | Left go to the next | previous workspace; nothing happens with one", async () => {
    await open();
    const stop = switcher.start(mock);
    await until(() => switcher.list.length === 1, "one workspace");
    press(window, "ArrowRight", { ctrlKey: true, altKey: true });
    expect(mock.probe.shell.focused).toEqual([]);
    list();
    await until(() => switcher.list.length === 4, "four");
    const command = (id: string) => contributions.of("command").find((c) => c.id === id)!;
    await command("workspace.next").run();
    await command("workspace.previous").run();
    expect(mock.probe.shell.focused).toEqual([90, 92]);
    const keys = contributions.of("shortcut").filter((s) => s.command.startsWith("workspace."));
    expect(keys.map((k) => [k.command, k.keys])).toEqual([
      ["workspace.next", "Mod+Alt+ArrowRight"],
      ["workspace.previous", "Mod+Alt+ArrowLeft"],
    ]);
    stop();
  });
});

describe("the welcome screen", () => {
  it("offers Open folder, Open file and New project, lists the recent folders with their age", async () => {
    await store.start().catch(() => {});
    started = true;
    await mock.showWelcome();
    await until(() => store.view.state === "welcome" && store.recent.length > 0, "welcome");
    component = mount(Welcome, { target, props: { store } });
    flushSync();
    const buttons = Array.from(target.querySelectorAll(".actions .btn")).map((b) =>
      b.textContent?.replace(/\s+/g, " ").trim(),
    );
    expect(buttons[0]).toContain("Open folder");
    expect(buttons[1]).toContain("Open file");
    expect(buttons[2]).toBe("New project…");
    const rows = target.querySelectorAll('[role="option"]');
    expect(rows).toHaveLength(2);
    expect(rows[0]?.textContent).toContain("permark-docs");
    expect(rows[0]?.querySelector(".age")?.textContent).toMatch(/now|hour/);
    // a folder that is gone is marked and removable, and does not open
    expect(rows[1]?.textContent).toContain("(not found)");
    expect(rows[1]?.querySelector("button")?.textContent).toBe("Remove");
    expect(target.textContent).toContain("Drop a folder or a .md file anywhere to open it.");
  });

  it("New project picks a folder, initializes it and opens it as a project", async () => {
    await store.start().catch(() => {});
    started = true;
    await mock.showWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    component = mount(Welcome, { target, props: { store } });
    flushSync();
    byText("button", "New project").click();
    await until(() => store.view.state === "showing", "opened");
    expect(mock.probe.shell.project().initialized).toBe(true);
    await until(() => conditions.current.permark === "initialized", "initialized");
  });
});
