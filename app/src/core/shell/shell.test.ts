// @vitest-environment jsdom
// The shell's own screens against the mock backend: the settings page (sections, plugins, project settings), the
// initialize question, the top bar (chips, Open menu, closing) and its keys, the welcome screen.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";
import { sizeText } from "../i18n";

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
const { default: PanelToggle } = await import("./PanelToggle.svelte");
const { layout } = await import("./layout.svelte");
const { arrangement } = await import("./arrangement.svelte");
const { defaultArrangement, moveTo } = await import("./arrangement");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { closeHere } = await import("./closeWorkspace");
const { default: CloseWindowDialog } = await import("./CloseWindowDialog.svelte");
const { default: Welcome } = await import("./Welcome.svelte");
const { ui } = await import("./ui.svelte");
const { windowStore } = await import("../window.svelte");
const { tabs } = await import("./tabs.svelte");
const { registerCoreCommands } = await import("../commands/core");
const { installWindowKeys } = await import("../commands/install");
const { contributions, host } = await import("../plugins/instance.svelte");
const { conditions } = await import("../conditions/context.svelte");
const { pluginsStore, projectSettings } = await import("./settings/stores.svelte");
const { confirmInit, askInitHere, newProject, answerInit } = await import("./initProject");
const { default: Notices } = await import("./Notices.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const store = windowStore;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let unregister: (() => void) | null = null;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
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

/** The nav's groups: each label with the names of its sections, in order. */
function groups(): [string, string[]][] {
  return Array.from(target.querySelectorAll<HTMLElement>('.nav [role="group"]')).map((g) => [
    g.getAttribute("aria-label") ?? "",
    Array.from(g.querySelectorAll(".nav-item")).map((b) => b.textContent?.trim() ?? ""),
  ]);
}

function mountPage(
  section: "reading" | "editing" | "integration" | "plugins" | "project" | "files" | "pluginsHere",
): void {
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
  for (const id of [
    "git",
    "unread",
    "review",
    "notes",
    "agent",
    "token-count",
    "updates",
    "shell-integration",
    "spelling",
  ]) {
    await store.changeSetting({ key: "pluginEnabled", value: { plugin: id, enabled: null } });
  }
  mock.probe.shell.setOthers([]);
  mock.probe.shell.forgetInit();
  mock.probe.shell.removed.length = 0;
  mock.probe.shell.focused.length = 0;
  mock.probe.tabs.moved.length = 0;
  mock.probe.tabs.refuseMove = false;
  await toWelcome();
});

describe("the settings page", () => {
  it("groups the nav: App (Reading, Editing, Integration, Plugins), This folder only while a project is shown", async () => {
    await store.start().catch(() => {});
    started = true;
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    mountPage("reading");
    const names = () =>
      Array.from(target.querySelectorAll(".nav-item")).map((b) => b.textContent?.trim());
    expect(names()).toEqual(["Reading", "Editing", "Integration", "Plugins"]);
    expect(groups()).toEqual([["App", ["Reading", "Editing", "Integration", "Plugins"]]]);
    // a window with no folder: the page is the app's settings, plainly named
    expect(target.querySelector(".bar h2")?.textContent).toBe("Settings");
    // width and zoom live in the document header's reading menu, not here
    expect(target.querySelector("#set-width")).toBeNull();
    expect(target.querySelector("#set-zoom")).toBeNull();
    await unmount(component!);
    component = null;
    await open();
    mountPage("reading");
    await tick();
    expect(names()).toEqual([
      "Reading",
      "Editing",
      "Integration",
      "Plugins",
      "Overview",
      "Files",
      "Plugins here",
    ]);
    expect(groups()).toEqual([
      ["App", ["Reading", "Editing", "Integration", "Plugins"]],
      ["This folder", ["Overview", "Files", "Plugins here"]],
    ]);
    // each section wears its icon, decoration only (the name is the words)
    const icons = Array.from(target.querySelectorAll(".nav-item")).map((b) =>
      Array.from(b.querySelector("svg")?.classList ?? []).find(
        (c) => c.startsWith("lucide-") && c !== "lucide-icon",
      ),
    );
    expect(icons).toEqual([
      "lucide-book-open",
      "lucide-pen-line",
      "lucide-plug",
      "lucide-puzzle",
      "lucide-folder-cog",
      "lucide-files",
      "lucide-blocks",
    ]);
    expect(target.querySelector(".nav-item svg")?.getAttribute("aria-hidden")).toBe("true");
  });

  it("the title names the folder shown; the dialog is named the same", async () => {
    await open();
    const name = store.view.state === "showing" ? store.view.workspace.name : "";
    mountPage("reading");
    const title = `Settings — ${name}`;
    expect(target.querySelector(".bar h2")?.textContent).toBe(title);
    expect(target.querySelector(".page")?.getAttribute("aria-label")).toBe(title);
  });

  it("a plugin's page sits in the group it asks for (App unless it says This folder)", async () => {
    await open();
    const page = (
      id: `${string}.${string}`,
      label: "agent.section" | "cmd.keyboard",
      order: number,
      group?: "app" | "folder",
    ) =>
      contributions.register({
        kind: "settingsPage",
        id,
        owner: "test",
        requires: NO_REQUIREMENT,
        label,
        order,
        ...(group ? { group } : {}),
        view: () => import("./settings/IntegrationSection.svelte"),
      });
    const removals = [
      page("test.app", "cmd.keyboard", 30),
      page("test.folder", "agent.section", 30, "folder"),
    ];
    try {
      mountPage("reading");
      await tick();
      expect(groups()).toEqual([
        ["App", ["Reading", "Editing", "Keyboard shortcuts…", "Integration", "Plugins"]],
        ["This folder", ["Overview", "Files", "AI agents", "Plugins here"]],
      ]);
    } finally {
      for (const remove of removals) remove();
    }
  });

  it("Open this folder's settings: only with a project; opens the page on the folder's first section", async () => {
    await store.start().catch(() => {});
    started = true;
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    const listed = () =>
      contributions
        .visible("command", conditions.current)
        .some((c) => c.id === "app.folderSettings");
    expect(listed()).toBe(false);
    await open();
    expect(listed()).toBe(true);
    const command = contributions.of("command").find((c) => c.id === "app.folderSettings")!;
    expect(command.label).toBe("cmd.folderSettings");
    command.run();
    expect(ui.settingsPage).toEqual({ status: "open", section: "project" });
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

  it("opened from the quick settings (gone by then): closing it puts the keyboard on the gear", async () => {
    await open();
    const gear = document.createElement("button");
    gear.setAttribute("data-settings-trigger", "");
    const allSettings = document.createElement("button");
    document.body.append(gear, allSettings);
    allSettings.focus();
    mountPage("reading");
    // the popover holding "All settings" closes as the page opens
    allSettings.remove();
    press(target.querySelector<HTMLElement>(".page")!, "Escape");
    await unmount(component!);
    component = null;
    expect(document.activeElement).toBe(gear);
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
    // from the first section up to the last one: the folder's plugins
    expect(target.querySelector("h3.title")?.textContent).toBe("Plugins here");
    // the arrows cross from one group into the other (the app's last section to the folder's first)
    ui.showSettingsSection("plugins");
    await tick();
    target.querySelector<HTMLElement>('[aria-current="page"]')!.focus();
    press(document.activeElement!, "ArrowDown");
    await tick();
    expect(document.activeElement?.textContent?.trim()).toBe("Overview");
    // Home | End: the first | last section of the list
    press(document.activeElement!, "End");
    await tick();
    expect(document.activeElement?.textContent?.trim()).toBe("Plugins here");
    press(document.activeElement!, "Home");
    await tick();
    expect(document.activeElement?.textContent?.trim()).toBe("Reading");
    expect(target.querySelector("h3.title")?.textContent).toBe("Reading");
  });

  it("the section shown is scrolled into view in the section list (a row of tabs when narrow)", async () => {
    await open();
    const scrolled = vi.spyOn(Element.prototype, "scrollIntoView");
    mountPage("reading");
    ui.showSettingsSection("pluginsHere");
    await tick();
    const current = target.querySelector('[aria-current="page"]');
    expect(current?.textContent?.trim()).toBe("Plugins here");
    expect(scrolled.mock.contexts.at(-1)).toBe(current);
    expect(scrolled.mock.calls.at(-1)?.[0]).toEqual({ block: "nearest", inline: "nearest" });
    scrolled.mockRestore();
  });

  it("the group names show only while there are two groups (no folder: no App label)", async () => {
    await open();
    mountPage("reading");
    expect(target.querySelectorAll(".nav-label")).toHaveLength(2);
    await unmount(component!);
    component = null;
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    mountPage("reading");
    expect(groups().map(([name]) => name)).toEqual(["App"]);
    expect(target.querySelectorAll(".nav-label")).toHaveLength(0);
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
    const shown = () =>
      Array.from(target.querySelectorAll("section.group h4")).map((h) => h.textContent?.trim());
    // plugin sections arrive as their plugins activate, in no fixed order: wait for the ones checked below
    const expected = ["File manager", "Updates", "Git", "Notes"];
    await until(
      () => expected.every((name) => shown().includes(name)),
      `groups ${shown().join(",")}`,
    );
    const headings = shown();
    expect(headings[0]).toBe("Default app");
    expect(headings).toEqual(expect.arrayContaining(expected));
    // the agent connection has its own section (Settings > AI agents)
    expect(headings).not.toContain("Agent connection");
    // Windows: who opens the files now, and the way to Windows' own page
    await until(
      () => target.textContent?.includes("Choose in Windows Settings") === true,
      "handoff",
    );
    expect(target.textContent).toContain("Opens with PerMark");
    // plugins off: their sections go, the default app stays. Every plugin that is on has loaded first, so the owners
    // read here are all of them: a section of a plugin left out of a fixed list (one that loaded late) never went.
    await until(
      () => conditions.current.plugins.every((id) => host.state(id) !== "loading"),
      "the plugins loaded",
    );
    const owners = new Set(
      contributions.visible("settingsSection", conditions.current).map((c) => c.owner),
    );
    for (const id of owners) {
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

  it("every plugin is listed once, grouped by kind, with its switch on unless off by default", async () => {
    await pluginsPage();
    expect(target.querySelectorAll(".plugin")).toHaveLength(42);
    const groups = Array.from(target.querySelectorAll("section.group h4")).map(
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
    // every plugin starts on, the specialist tools too; only the experimental
    // Assistant (an outside agent working on a copy) waits for the person to turn it on
    const off = Array.from(target.querySelectorAll<HTMLInputElement>('input[role="switch"]'))
      .filter((i) => !i.checked)
      .map((i) => i.closest(".plugin")?.querySelector(".name")?.textContent?.trim());
    expect(off).toEqual(["Assistant"]);
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

  it("the question says how much space removing the data frees, when the plugin knows", async () => {
    await pluginsPage();
    const bytes = await mock.pluginDataBytes("local-ai");
    expect(bytes.ok && (bytes.value ?? 0) > 0).toBe(true);
    const size = sizeText(bytes.ok ? (bytes.value ?? 0) : 0);
    switchOf("Local AI").click();
    await until(
      () => row("Local AI").textContent?.includes("Remove data") === true,
      "off, with the way to remove its data",
    );
    Array.from(row("Local AI").querySelectorAll("button"))
      .find((b) => b.textContent?.trim() === "Remove data")
      ?.click();
    await until(
      () => row("Local AI").textContent?.includes(`This frees ${size}.`) === true,
      "the bytes join the question",
    );
    expect(row("Local AI").textContent).toContain(
      `Remove the data of Local AI? This frees ${size}.`,
    );
    byText("button", "Cancel").click();
    await tick();
    expect(mock.probe.shell.removed).not.toContain("local-ai");
    await store.changeSetting({
      key: "pluginEnabled",
      value: { plugin: "local-ai", enabled: null },
    });
  });

  it("a plugin of the project has only the app switch here, and its row says when this project turned it off", async () => {
    await pluginsPage();
    expect(row("Comments").querySelector("select")).toBeNull();
    await mock.pluginSetProject("comments", false);
    await until(() => /Off in this folder/.test(statusOf("Comments")), "off here");
    // the app's switch is untouched; the plugin is not offered for data removal
    expect(switchOf("Comments").checked).toBe(true);
    expect(row("Comments").textContent).not.toContain("Remove data");
    await mock.pluginSetProject("comments", null);
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

describe("plugins here", () => {
  async function herePage(): Promise<void> {
    mountPage("pluginsHere");
    await until(() => pluginsStore.state.status === "ready", "the list");
    await tick();
  }
  const select = (id: string): HTMLSelectElement | null =>
    target.querySelector<HTMLSelectElement>(`#here-${id}`);
  const rowOf = (id: string): HTMLElement => select(id)!.closest<HTMLElement>(".row")!;
  const chipOf = (id: string): string | null =>
    rowOf(id).querySelector(".scope-chip")?.getAttribute("data-scope") ?? null;
  function pick(id: string, value: string): void {
    const box = select(id)!;
    box.value = value;
    box.dispatchEvent(new Event("change", { bubbles: true }));
  }
  async function info(id: string): Promise<{ choice: boolean | null; teamOff: boolean }> {
    const listed = await mock.pluginsList();
    const found = listed.ok ? listed.value.find((p) => p.manifest.id === id) : undefined;
    return { choice: found?.projectChoice ?? null, teamOff: found?.projectConfigOff ?? false };
  }

  it("lists only the plugins that work per folder, each with four choices; Off for everyone waits for Initialize", async () => {
    await open();
    await herePage();
    expect(target.querySelector("h3.title")?.textContent).toBe("Plugins here");
    expect(select("comments")).not.toBeNull();
    expect(select("notes")).toBeNull();
    expect(select("mermaid")).toBeNull();
    const options = Array.from(select("comments")!.options).map((o) => [
      o.textContent?.trim(),
      o.disabled,
    ]);
    expect(options).toEqual([
      ["Same as the app", false],
      ["On here", false],
      ["Off here (only you)", false],
      ["Off for everyone (Initialize PerMark first)", true],
    ]);
    expect(target.textContent).toContain("Initialize PerMark to share this with everyone");
    expect(chipOf("comments")).toBeNull();
    pick("comments", "off");
    await vi.waitFor(async () => expect((await info("comments")).choice).toBe(false));
    await until(() => chipOf("comments") === "you", "the chip says Only you");
    expect(rowOf("comments").textContent).toContain("Off in this folder.");
    pick("comments", "app");
    await until(() => chipOf("comments") === null, "follows the app again");
    expect((await info("comments")).choice).toBeNull();
  });

  it("Off for everyone writes the folder's file and says so; Same as the app takes it back", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await herePage();
    expect(target.textContent).not.toContain("Initialize PerMark to share this with everyone");
    expect(Array.from(select("comments")!.options).every((o) => !o.disabled)).toBe(true);
    expect(select("comments")!.options[3]?.textContent?.trim()).toBe("Off for everyone");
    pick("comments", "offEveryone");
    await until(() => select("comments")?.value === "offEveryone", "shown as off for everyone");
    expect((await info("comments")).teamOff).toBe(true);
    expect(chipOf("comments")).toBe("everyone");
    expect(rowOf("comments").textContent).toContain(
      "Turned off for everyone in .permark/config.toml.",
    );
    pick("comments", "app");
    await vi.waitFor(async () =>
      expect(await info("comments")).toEqual({ choice: null, teamOff: false }),
    );
    await until(() => chipOf("comments") === null, "no chip: follows the app");
  });

  it("a refused Off for everyone says why and the box shows the choice still in force", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await herePage();
    const refuse = vi.spyOn(mock, "projectConfigSet").mockResolvedValueOnce({
      ok: false,
      error: {
        kind: "backend",
        command: "project_config_set",
        error: { kind: "invalidData", subject: "pluginOff", detail: "denied" },
      },
    });
    pick("comments", "offEveryone");
    await until(() => rowOf("comments").querySelector(".problem") !== null, "says why");
    await until(() => select("comments")?.value === "app", "back on the choice in force");
    expect((await info("comments")).teamOff).toBe(false);
    refuse.mockRestore();
  });

  it("leaving Off for everyone keeps the person's choice first, then frees the folder's switch", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await herePage();
    pick("comments", "offEveryone");
    await until(() => chipOf("comments") === "everyone", "off for everyone");
    const order: string[] = [];
    const person = vi.spyOn(mock, "pluginSetProject");
    const team = vi.spyOn(mock, "projectConfigSet");
    person.mockImplementation(async (...args) => {
      order.push("person");
      person.mockRestore();
      return mock.pluginSetProject(...args);
    });
    team.mockImplementation(async (...args) => {
      order.push("team");
      team.mockRestore();
      return mock.projectConfigSet(...args);
    });
    pick("comments", "on");
    await until(() => order.length === 2, "both written");
    expect(order).toEqual(["person", "team"]);
    await vi.waitFor(async () =>
      expect(await info("comments")).toEqual({ choice: true, teamOff: false }),
    );
    await until(() => chipOf("comments") === "you", "the chip says Only you");
  });

  it("a refused choice of the person leaves the plugin off for everyone (the safer state)", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await herePage();
    pick("comments", "offEveryone");
    await until(() => chipOf("comments") === "everyone", "off for everyone");
    const refuse = vi.spyOn(mock, "pluginSetProject").mockResolvedValueOnce({
      ok: false,
      error: {
        kind: "backend",
        command: "plugin_set_project",
        error: { kind: "io", subject: null, detail: null },
      },
    });
    const team = vi.spyOn(mock, "projectConfigSet");
    pick("comments", "on");
    await until(() => refuse.mock.calls.length === 1, "the person's choice asked");
    await until(() => select("comments")?.value === "offEveryone", "still off for everyone");
    expect(team).not.toHaveBeenCalled();
    expect((await info("comments")).teamOff).toBe(true);
    refuse.mockRestore();
    team.mockRestore();
  });
});

describe("the project settings and Initialize PerMark", () => {
  it("Overview: says who each kind of setting is for; not initialized, it offers to set PerMark up, in plain words", async () => {
    await open();
    mountPage("project");
    await tick();
    expect(target.querySelector("h3.title")?.textContent).toBe("Overview");
    // nothing is marked Everyone before Initialize: the sentence about the marks waits
    expect(target.textContent).not.toContain("Settings marked Everyone");
    expect(target.textContent).toContain("This folder has no PerMark settings yet");
    // the full offer lives here only
    expect(target.querySelector(".share-init:not(.compact)")).not.toBeNull();
    expect(target.textContent).not.toMatch(/not initialized/);
    // the folder's files live in their own section
    expect(target.querySelector(".list")).toBeNull();
    const init = byText("button", "Initialize PerMark");
    init.focus();
    init.click();
    expect(ui.initDialog.status).toBe("asking");
    await confirmInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await until(
      () => !target.textContent?.includes("This folder has no PerMark settings yet"),
      "the offer gone",
    );
    expect(ui.initDialog.status).toBe("closed");
    expect(target.textContent).toContain(
      "Settings marked Everyone are kept in this folder's .permark/config.toml",
    );
    expect(target.textContent).toContain("Settings marked Only you stay on this computer.");
    // the button that had the keyboard is gone with the offer: the section's heading takes it, not the body
    await until(
      () => document.activeElement === target.querySelector("h3.title"),
      "on the heading",
    );
  });

  it("Overview, set up: whether git sees the folder's PerMark settings is chosen there for everyone; a file someone else wrote is only described", async () => {
    const label = "Let git see this folder's PerMark settings";
    await open();
    mountPage("project");
    await tick();
    expect(target.textContent).not.toContain(label);
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await until(() => target.textContent?.includes(label) ?? false, "the row");
    const row = (): HTMLElement => byText(".row", label);
    expect(row().querySelector(".name")?.textContent).toContain("Everyone");
    expect(row().textContent).toContain("Your private data and caches always stay out of git.");
    const radio = (name: string): HTMLButtonElement | undefined =>
      Array.from(row().querySelectorAll<HTMLButtonElement>("button[role=radio]")).find(
        (b) => b.textContent?.trim() === name,
      );
    expect(radio("On")?.getAttribute("aria-checked")).toBe("true");
    radio("Off")?.click();
    await until(() => mock.probe.shell.project().sharing === "private", "kept");
    await until(() => radio("Off")?.getAttribute("aria-checked") === "true", "shown");
    // someone else rewrote the file meanwhile: the change is refused, says why, and the row describes the file
    mock.probe.shell.sharing("custom");
    radio("On")?.click();
    await until(
      () => target.textContent?.includes("Couldn't change what git sees in .permark") ?? false,
      "why",
    );
    await until(() => row().querySelector("button[role=radio]") === null, "no choice left");
    expect(row().textContent).toContain("was written by someone else");
    expect(mock.probe.shell.project().sharing).toBe("custom");
  });

  it("Files, not initialized: the shared rows show greyed and do nothing until the person sets PerMark up", async () => {
    await open();
    mountPage("files");
    await until(() => projectSettings.state.status === "ready", "loaded");
    await tick();
    expect(target.querySelector("h3.title")?.textContent).toBe("Files");
    expect(target.textContent).toContain("Initialize PerMark to share this with everyone");
    // one line here, not the Overview's full offer
    expect(target.querySelector(".share-init.compact")).not.toBeNull();
    expect(target.textContent).not.toContain("This folder has no PerMark settings yet");
    const fields = () => Array.from(target.querySelectorAll<HTMLInputElement>(".list input"));
    expect(fields().length).toBe(3);
    expect(fields().every((f) => f.disabled)).toBe(true);
    expect(
      Array.from(target.querySelectorAll<HTMLButtonElement>("button[role=radio]")).every(
        (b) => b.disabled,
      ),
    ).toBe(true);
    expect(target.querySelector(".shared.off")).not.toBeNull();
    // nothing is written until the person answers the question
    byText("button", "Initialize PerMark").click();
    expect(ui.initDialog.status).toBe("asking");
    expect(mock.probe.shell.project().initialized).toBe(false);
    await confirmInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await until(() => fields().every((f) => !f.disabled), "the rows work");
    expect(target.textContent).not.toContain("Initialize PerMark to share this with everyone");
    expect(target.querySelector(".shared.off")).toBeNull();
  });

  it("patterns are added and removed one by one, kept at once; a refused value says why", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    mountPage("files");
    await until(() => projectSettings.state.status === "ready", "loaded");
    await tick();
    // every value here is shared through the folder's config file and says so, in the row's name
    const names = Array.from(target.querySelectorAll(".shared .row .name")).map(
      (n) => n.textContent ?? "",
    );
    expect(names.length).toBe(4);
    expect(names.every((n) => n.includes("Everyone, kept in .permark/config.toml"))).toBe(true);
    expect(names.map((n) => n.split("Everyone")[0]?.trim())).toEqual([
      "Files shown by default",
      "Extra files to show",
      "Files to hide",
      "Left out of search and live feed",
    ]);
    const input = target.querySelector<HTMLInputElement>(
      'input[aria-label="Left out of search and live feed"]',
    )!;
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

  it("Overview, initialized: names the shared file and shows it in the file manager; Files holds no agent rows", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await until(
      () =>
        contributions.visible("settingsPage", conditions.current).some((p) => p.owner === "agent"),
      "the agent plugin is on",
    );
    mountPage("project");
    await until(
      () => target.textContent?.includes(".permark/config.toml") === true,
      "the file row",
    );
    expect(target.textContent).toContain("Shared with whoever opens this folder.");
    mock.probe.opened.length = 0;
    byText("button", "Show in file manager").click();
    await until(() => mock.probe.opened.length === 1, "revealed");
    expect(mock.probe.opened).toEqual([{ path: ".permark/config.toml", mode: "reveal" }]);
    ui.showSettingsSection("files");
    await until(() => projectSettings.state.status === "ready", "loaded");
    await tick();
    expect(target.querySelector("h3.title")?.textContent).toBe("Files");
    const content = target.querySelector(".content")?.textContent ?? "";
    expect(content).not.toContain("Groups agents never get here");
    expect(content).not.toContain("Tokenizers for this folder");
  });

  it("a plugin key the folder's file sets is said on the app row it does not change, in a quiet line", async () => {
    await open();
    await mock.projectInit(MOCK_FOLDER);
    await until(() => conditions.current.permark === "initialized", "initialized");
    await until(
      () =>
        contributions.visible("settingsSection", conditions.current).some((c) => c.owner === "git"),
      "the git row",
    );
    const line = "Set in .permark/config.toml; not used here — your choice applies.";
    mountPage("integration");
    await until(() => target.querySelector("#set-git") !== null, "the git row shows");
    await until(() => projectSettings.state.status === "ready", "loaded");
    await tick();
    expect(target.textContent).not.toContain(line);
    await unmount(component!);
    component = null;
    mock.probe.shell.ignorePluginKeys(["git.backend"]);
    const notices = store.notices.length;
    mountPage("integration");
    await until(() => target.textContent?.includes(line) === true, "the line");
    const row = byText(".row", line);
    expect(row.querySelector("#set-git")).not.toBeNull();
    expect(row.querySelector(".note")?.textContent?.trim()).toBe(line);
    // no notice: the line is all there is
    expect(store.notices.length).toBe(notices);
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
    // a stray Enter writes nothing: the keyboard starts on Cancel
    expect(document.activeElement?.textContent?.trim()).toBe("Cancel");
    expect(target.textContent).toContain("Initialize PerMark in permark-docs?");
    expect(target.textContent).toContain(".permark/config.toml");
    expect(target.textContent).toContain("Your files are not touched.");
    // the undo history moves with the private data; what the person allows for the folder stays behind
    expect(target.textContent).toContain("(comments, read marks, undo history)");
    expect(target.textContent).toContain(
      "such as agent access, stays in your own PerMark data on this computer.",
    );
    press(target.querySelector(".dialog")!, "Escape");
    byText("button", "Cancel").click();
    byText("button.btn--primary", "Initialize PerMark").click();
    expect(answers).toEqual([false, false, true]);
    ui.closeInit();
  });

  it("New project on a folder with files in it asks first; yes sets it up and opens it", async () => {
    await open();
    await newProject();
    expect(ui.initDialog).toEqual({
      status: "asking",
      path: MOCK_FOLDER,
      name: "permark-docs",
      opens: true,
    });
    await confirmInit(MOCK_FOLDER, true);
    expect(ui.initDialog.status).toBe("closed");
    await until(() => conditions.current.permark === "initialized", "initialized");
    expect(store.view.state === "showing" && store.view.workspace.name).toBe("permark-docs");
  });

  it("New project, the question on a folder with files: plain words, the safe button goes back to choosing", async () => {
    await open();
    component = mount(InitDialog, {
      target,
      props: { name: "permark-docs", path: MOCK_FOLDER, fresh: true, answer: () => undefined },
    });
    flushSync();
    expect(target.textContent).toContain("Make permark-docs a project?");
    expect(target.textContent).toContain("This folder already has files in it.");
    expect(target.textContent).not.toContain(".gitignore");
    expect(document.activeElement?.textContent?.trim()).toBe("Choose another folder…");
    await unmount(component);
    component = null;
    // no: the folder picker again, starting at the folder chosen
    await newProject();
    expect(ui.initDialog.status).toBe("asking");
    answerInit({ path: MOCK_FOLDER, opens: true }, false);
    await until(() => mock.probe.pickedFrom.at(-1) === MOCK_FOLDER, "picker at the folder");
    await until(() => ui.initDialog.status === "asking", "asked again");
    ui.closeInit();
  });

  it("a notice that waits for the person: Esc with the keyboard in it dismisses it", async () => {
    const gone: string[] = [];
    component = mount(Notices, { target, props: { store } });
    store.notifyLasting({ text: "Review closed.", actions: [], onGone: (how) => gone.push(how) });
    flushSync();
    const mine = byText("[role=group]", "Review closed.");
    const dismiss = mine.querySelector<HTMLButtonElement>("button")!;
    dismiss.focus();
    const esc = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    dismiss.dispatchEvent(esc);
    flushSync();
    expect(esc.defaultPrevented).toBe(true);
    expect(gone).toEqual(["dismissed"]);
    expect(target.textContent).not.toContain("Review closed.");
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
        closing: false,
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

  it("tab keys switch at once, also from a focused chip, and leave no chip focused (no stray focus ring)", async () => {
    const bar = await mountBar();
    const stopKeys = installWindowKeys();
    const focusedChip = (): boolean =>
      document.activeElement instanceof HTMLElement && document.activeElement.matches("[data-tab]");
    const nextOf = (step: 1 | -1): number => {
      const at = tabs.list.findIndex((t) => t.tab === tabs.active);
      return tabs.list[(at + step + tabs.list.length) % tabs.list.length]!.tab;
    };
    // Ctrl+Alt+Right on a focused chip switches the tab (the chip's arrow keys take plain arrows only)
    const chip = chips().find((c) => c.tabIndex === 0)!;
    chip.focus();
    let want = nextOf(1);
    press(chip, "ArrowRight", { ctrlKey: true, altKey: true });
    await until(() => tabs.active === want, "Ctrl+Alt+Right switched");
    flushSync();
    expect(focusedChip()).toBe(false);
    // Ctrl+Shift+Tab | Ctrl+Tab | Ctrl+n from the page: switched, no chip takes the focus
    want = nextOf(-1);
    press(window, "Tab", { ctrlKey: true, shiftKey: true });
    await until(() => tabs.active === want, "Ctrl+Shift+Tab switched");
    want = nextOf(1);
    press(document.body, "Tab", { ctrlKey: true });
    await until(() => tabs.active === want, "Ctrl+Tab switched");
    press(document.body, "1", { ctrlKey: true });
    await until(() => tabs.active === tabs.list[0]!.tab, "Ctrl+1 switched");
    flushSync();
    expect(focusedChip()).toBe(false);
    // a chip picked with Enter lets go of the focus once its tab is the active one
    const third = chips()[2]!;
    third.focus();
    third.click();
    await until(() => tabs.active === tabs.list[2]!.tab, "picked");
    flushSync();
    expect(focusedChip()).toBe(false);
    // a mouse press on a chip never takes the focus from the page
    const down = new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 });
    chips()[1]!.dispatchEvent(down);
    expect(down.defaultPrevented).toBe(true);
    // one active chip (it joins the page below): its item carries the look, its button says so
    const current = Array.from(target.querySelectorAll(".chip.is-current"));
    expect(current.length).toBe(1);
    expect(current[0]?.querySelector("[data-tab]")?.getAttribute("aria-current")).toBe("page");
    expect(current[0]?.getAttribute("data-id")).toBe(String(tabs.active));
    // plain arrows still move between chips
    chips()[0]!.focus();
    press(chips()[0]!, "ArrowRight");
    expect(document.activeElement).toBe(chips()[1]);
    stopKeys();
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
    // a move that cannot happen: the tab stays, a quiet notice says so
    mock.probe.tabs.refuseMove = true;
    chips()[1]!.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 40, clientY: 30 }),
    );
    await tick();
    (document.querySelectorAll('[role="menuitem"]')[0] as HTMLElement).click();
    await until(
      () => windowStore.notices.some((n) => n.kind === "note" && n.text.includes("could not move")),
      "move refused notice",
    );
    expect(chips().length).toBe(3);
    mock.probe.tabs.refuseMove = false;
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

  it("Move to window lists the other windows (focused last first) in place and moves the tab there", async () => {
    const bar = await mountBar();
    const summary = (tab: number, title: string) => ({
      tab,
      kind: "project" as const,
      title,
      path: `C:/${title}`,
      loaded: true,
      unsaved: false,
    });
    mock.probe.shell.setOthers([
      {
        window: "w-2",
        closing: false,
        tabs: [summary(70, "older"), summary(71, "older-b")],
        active: 71,
        focusedMs: 10,
      },
      { window: "w-3", closing: false, tabs: [summary(80, "newest")], active: 80, focusedMs: 30 },
      { window: "w-4", closing: false, tabs: [], active: null, focusedMs: 20 },
      // asking to close: takes no tab, not listed
      { window: "w-5", closing: true, tabs: [summary(90, "leaving")], active: 90, focusedMs: 99 },
    ]);
    await until(() => tabs.others.length === 3, "the other windows");
    const items = () =>
      Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).map((i) =>
        i.textContent?.replace(/\s+/g, " ").trim(),
      );
    const openMenu = async (chip: Element) => {
      chip.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: 40,
          clientY: 30,
        }),
      );
      await tick();
    };
    await openMenu(chips()[1]!);
    expect(items()).toEqual(["Move to new window", "Move to window ▸", "Close"]);
    const entry = document.querySelectorAll<HTMLElement>('[role="menuitem"]')[1]!;
    expect(entry.getAttribute("aria-haspopup")).toBe("menu");
    // the submenu, in place: the windows by the tab they show + their tab count, focused last first
    entry.click();
    await until(() => items()[0] === "newest 1 tab", "the submenu");
    expect(items()).toEqual(["newest 1 tab", "Empty window 0 tabs", "older-b 2 tabs"]);
    expect(document.querySelector('[role="menu"] .heading')?.textContent?.trim()).toBe(
      "Move to window",
    );
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "Move to window",
    );
    // ArrowLeft goes back with the keyboard on "Move to window ▸"; ArrowRight opens it again
    press(document.querySelector('[role="menu"]')!, "ArrowLeft");
    await until(() => items()[0] === "Move to new window", "back");
    flushSync();
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe("Tab actions");
    expect(
      document
        .querySelector('[role="menuitem"].is-active')
        ?.textContent?.replace(/\s+/g, " ")
        .trim(),
    ).toBe("Move to window ▸");
    press(document.querySelector('[role="menu"]')!, "ArrowRight");
    await until(() => items()[0] === "newest 1 tab", "the submenu by key");
    // a pick moves the tab there
    const moving = tabs.list[1]!.tab;
    mock.probe.tabs.movedTo.length = 0;
    document.querySelectorAll<HTMLElement>('[role="menuitem"]')[2]!.click();
    await until(() => mock.probe.tabs.movedTo.length === 1, "moved");
    expect(mock.probe.tabs.movedTo).toEqual([[moving, "w-2"]]);
    await until(() => chips().length === 3, "gone from here");
    // refused: the tab stays, a quiet notice says so
    mock.probe.tabs.refuseMove = true;
    await openMenu(chips()[1]!);
    document.querySelectorAll<HTMLElement>('[role="menuitem"]')[1]!.click();
    await until(() => items()[0] === "newest 1 tab", "the submenu again");
    document.querySelectorAll<HTMLElement>('[role="menuitem"]')[0]!.click();
    await until(
      () => windowStore.notices.some((n) => n.kind === "note" && n.text.includes("to that window")),
      "move refused notice",
    );
    expect(chips().length).toBe(3);
    mock.probe.tabs.refuseMove = false;
    // one tab left: Move to window is still offered (the window it leaves closes); no other window: not offered
    await mock.tabClose(bar.ids[2]!);
    await mock.tabClose(bar.ids[3]!);
    await until(() => chips().length === 1, "one");
    await openMenu(chips()[0]!);
    expect(items()).toEqual(["Move to window ▸", "Close Ctrl+W"]);
    press(document.querySelector('[role="menu"]')!, "Escape");
    await tick();
    mock.probe.shell.setOthers([]);
    await until(() => tabs.others.length === 0, "alone");
    await openMenu(chips()[0]!);
    expect(items()).toEqual(["Close Ctrl+W"]);
    bar.stop();
  });

  it("a second move of a tab while the first is under way asks nothing (no double move)", async () => {
    const bar = await mountBar();
    const tab = bar.ids[1]!;
    const first = tabs.moveToNewWindow(tab);
    const second = tabs.moveToNewWindow(tab);
    expect(await second).toBe("busy");
    expect(await first).toBe("moved");
    expect(mock.probe.tabs.moved).toEqual([tab]);
    // a refused move frees the tab too: asking again asks the backend again
    mock.probe.tabs.refuseMove = true;
    expect(await tabs.moveToNewWindow(bar.ids[2]!)).toBe("refused");
    expect(await tabs.moveToNewWindow(bar.ids[2]!)).toBe("refused");
    mock.probe.tabs.refuseMove = false;
    // into another window: the same (one ask at a time, across both kinds of move); never into its own window
    const other = bar.ids[3]!;
    mock.probe.tabs.movedTo.length = 0;
    const into = tabs.moveToWindow(other, "w-2");
    const again = [tabs.moveToNewWindow(other), tabs.moveToWindow(other, "w-2")];
    expect(await Promise.all(again)).toEqual(["busy", "busy"]);
    expect(await into).toBe("moved");
    expect(mock.probe.tabs.movedTo).toEqual([[other, "w-2"]]);
    expect(await tabs.moveToWindow(bar.ids[2]!, "main")).toBe("refused");
    expect(mock.probe.tabs.movedTo.length).toBe(1);
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
      "Open folder… Ctrl+O",
      "Open file… Ctrl+Shift+O",
      "New project…",
      "Notes",
      "api-specs C:/work/api-specs",
      `permark-docs ${MOCK_FOLDER}`,
    ]);
    expect(menu.querySelector(".heading")?.textContent).toBe("Recent folders");
    // the same icons as the Welcome's buttons; Notes as their button; a folder per recent one
    const icons = Array.from(menu.querySelectorAll('[role="menuitem"]')).map((i) =>
      Array.from(i.querySelector("svg")?.classList ?? []).find(
        (c) => c.startsWith("lucide-") && c !== "lucide-icon",
      ),
    );
    expect(icons).toEqual([
      "lucide-folder-open",
      "lucide-file-text",
      "lucide-folder-plus",
      "lucide-notebook-pen",
      "lucide-folder",
      "lucide-folder",
    ]);
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

  it("the Notes button sits left of Settings: one click opens | brings forward the Notes tab; gone with the plugin", async () => {
    const bar = await mountBar();
    const find = (): HTMLElement | null =>
      target.querySelector<HTMLElement>("button.icon-btn.notes");
    await until(() => find() !== null, "the button");
    const notesButton = find()!;
    const settings = target.querySelector<HTMLElement>("[data-settings-trigger]")!;
    // left of Settings, after the flexible space
    expect(notesButton.nextElementSibling).toBe(settings);
    expect(notesButton.getAttribute("aria-label")).toBe("Notes");
    expect(notesButton.getAttribute("title")).toBe("Notes (Ctrl+Shift+D)");
    expect(notesButton.hasAttribute("aria-current")).toBe(false);
    expect(notesButton.hasAttribute("aria-pressed")).toBe(false);
    // a notebook and, where the bar has room (CSS), the word: the way to the Notes reads from the screen
    expect(notesButton.querySelector("svg.lucide-notebook-pen")).not.toBeNull();
    expect(notesButton.querySelector(".word")?.textContent).toBe("Notes");
    const openNotes = vi.spyOn(store, "openNotes");
    notesButton.click();
    expect(openNotes).toHaveBeenCalledWith("tab");
    await until(
      () => store.view.state === "showing" && store.view.workspace.kind === "notes",
      "the Notes tab forward",
    );
    // one place: the Notes tab that was there comes forward, no second one
    expect(tabs.list).toHaveLength(4);
    await until(() => find()?.getAttribute("aria-current") === "page", "current in the Notes tab");
    const keys = contributions
      .of("shortcut")
      .filter((k) => k.command.startsWith("notes."))
      .map((k) => [k.command, k.keys]);
    expect(keys).toEqual(
      expect.arrayContaining([
        ["notes.open", "Mod+Shift+d"],
        ["notes.new", "Mod+Shift+n"],
        ["notes.newHere", "Mod+n"],
      ]),
    );
    // the plugin off: no button, no keys
    await store.changeSetting({ key: "pluginEnabled", value: { plugin: "notes", enabled: false } });
    await until(() => find() === null, "gone");
    expect(contributions.of("shortcut").some((k) => k.command.startsWith("notes."))).toBe(false);
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
      ["tab.next", "Mod+PageDown"],
      ["tab.next", "Mod+Alt+ArrowRight"],
      ["tab.previous", "Ctrl+Shift+Tab"],
      ["tab.previous", "Mod+PageUp"],
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

describe("the side panels", () => {
  afterEach(() => {
    layout.setSidebarHidden(false);
    layout.setRightHidden(false);
    layout.setBottomHidden(false);
    arrangement.arrange(defaultArrangement());
  });

  it("Ctrl+J and the status bar toggle hide | show the bottom panel; the focus in it moves to the page", async () => {
    await open();
    const stopKeys = installWindowKeys();
    // a view the person opened, arranged at the bottom
    const remove = contributions.register({
      kind: "panel",
      id: "probe.panel",
      owner: "live-feed",
      side: "right",
      label: "live.label",
      requires: NO_REQUIREMENT,
      view: () => import("../testing/StripFirst.svelte"),
    });
    layout.setOpen("probe.panel", true);
    arrangement.arrange(moveTo(arrangement.layout, "probe.panel", "bottom"));
    component = mount(PanelToggle, { target, props: { side: "bottom" } });
    flushSync();
    const toggle = target.querySelector<HTMLElement>("button.toggle")!;
    expect(toggle.getAttribute("aria-label")).toBe("Bottom panel");
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    expect(toggle.getAttribute("title")).toBe("Hide bottom panel (Ctrl+J)");
    // something in the bottom panel holds the focus
    const region = document.createElement("aside");
    region.dataset.region = "bottom";
    const inside = document.createElement("button");
    region.append(inside);
    const page = document.createElement("main");
    page.dataset.region = "page";
    const scroller = document.createElement("div");
    scroller.dataset.scroller = "";
    scroller.tabIndex = -1;
    page.append(scroller);
    document.body.append(page, region);
    inside.focus();
    press(window, "j", { ctrlKey: true });
    expect(layout.bottomHidden).toBe(true);
    expect([layout.sidebarHidden, layout.rightHidden]).toEqual([false, false]);
    expect(ui.frameMoving).toBe(true);
    expect(document.activeElement, "the page's scroller has the focus").toBe(scroller);
    flushSync();
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    expect(toggle.getAttribute("title")).toBe("Show bottom panel (Ctrl+J)");
    toggle.click();
    expect(layout.bottomHidden).toBe(false);
    // the focus elsewhere stays where it is
    scroller.focus();
    press(window, "j", { ctrlKey: true });
    expect(layout.bottomHidden).toBe(true);
    expect(document.activeElement).toBe(scroller);
    // the view closed: no bottom panel, the key does nothing
    layout.setBottomHidden(false);
    layout.setOpen("probe.panel", false);
    const command = contributions.of("command").find((c) => c.id === "panel.toggleBottom")!;
    expect(command.when?.()).toBe(false);
    press(window, "j", { ctrlKey: true });
    expect(layout.bottomHidden).toBe(false);
    region.remove();
    page.remove();
    remove();
    stopKeys();
  });

  it("Ctrl+B and the status bar toggle hide | show the sidebar; the focus in it moves to the page", async () => {
    await open();
    const stopKeys = installWindowKeys();
    component = mount(PanelToggle, { target, props: { side: "sidebar" } });
    flushSync();
    const toggle = target.querySelector<HTMLElement>("button.toggle")!;
    expect(toggle.getAttribute("aria-label")).toBe("Sidebar");
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    expect(toggle.getAttribute("title")).toBe("Hide sidebar (Ctrl+B)");
    // something in the sidebar holds the focus
    const region = document.createElement("div");
    region.dataset.region = "sidebar";
    const inside = document.createElement("button");
    region.append(inside);
    const page = document.createElement("main");
    page.dataset.region = "page";
    const scroller = document.createElement("div");
    scroller.dataset.scroller = "";
    scroller.tabIndex = -1;
    page.append(scroller);
    document.body.append(region, page);
    inside.focus();
    press(window, "b", { ctrlKey: true });
    expect(layout.sidebarHidden).toBe(true);
    expect(ui.frameMoving).toBe(true);
    expect(document.activeElement, "the page's scroller has the focus").toBe(scroller);
    flushSync();
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    expect(toggle.getAttribute("title")).toBe("Show sidebar (Ctrl+B)");
    toggle.click();
    expect(layout.sidebarHidden).toBe(false);
    region.remove();
    page.remove();
    stopKeys();
  });

  it("an editor that takes Ctrl+B itself (bold) keeps it; a single file has no sidebar to hide", async () => {
    await open();
    const stopKeys = installWindowKeys();
    const editor = document.createElement("div");
    editor.addEventListener("keydown", (e) => e.preventDefault());
    document.body.append(editor);
    press(editor, "b", { ctrlKey: true });
    expect(layout.sidebarHidden).toBe(false);
    editor.remove();
    await mock.openPath("C:/p/one-file.md", "thisTab");
    await until(
      () => store.view.state === "showing" && store.view.workspace.kind === "singleFile",
      "the file shows",
    );
    press(window, "b", { ctrlKey: true });
    expect(layout.sidebarHidden).toBe(false);
    const command = contributions.of("command").find((c) => c.id === "panel.toggle")!;
    // nothing at the right it could show (no outline, no panel): not offered
    expect(command.when?.()).toBe(false);
    stopKeys();
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
    // a folder that is gone is marked and removable, and does not open; its Remove sits beside the option, not in it
    // (an option's content is one name to a screen reader: a button inside it is never reached)
    const gone = rows.find((r) => r.textContent?.includes("(not found)"));
    expect(gone).toBeDefined();
    expect(gone?.querySelector("button")).toBeNull();
    const list = target.querySelector<HTMLElement>('[role="listbox"]');
    const remove = Array.from(target.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Remove",
    );
    expect(remove).toBeDefined();
    expect(remove?.closest('[role="option"]')).toBeNull();
    // and outside the list box, which holds options only
    expect(list?.contains(remove ?? null)).toBe(false);
    // which folder it removes, told with the button
    const about = remove?.getAttribute("aria-describedby") ?? "";
    expect(document.getElementById(about)?.textContent).toBe("specs");
    expect(target.textContent).toContain("Drop folders or files here to open them.");
  });

  it("the recent folders list says its Delete key; with none, there is no list to land on", async () => {
    await store.start().catch(() => {});
    started = true;
    await toWelcome();
    await until(() => store.view.state === "welcome" && store.recent.length > 0, "welcome");
    component = mount(Welcome, { target, props: { store } });
    flushSync();
    const list = target.querySelector<HTMLElement>('[role="listbox"]');
    if (!list) throw new Error("no list box");
    expect(list.tabIndex).toBe(0);
    expect(list.getAttribute("aria-keyshortcuts")).toBe("Delete");
    const described = list.getAttribute("aria-describedby");
    const hint = described === null ? null : document.getElementById(described);
    expect(hint?.textContent).toContain("Delete");
    for (const recent of [...store.recent]) await store.removeRecent(recent.path);
    await until(() => store.recent.length === 0, "no recent folders");
    flushSync();
    expect(target.querySelector('[role="listbox"]')).toBeNull();
    expect(target.querySelector('.column [tabindex="0"]')).toBeNull();
    const empty = byText("p", "No recent folders yet.");
    expect(empty.closest("[role]")).toBeNull();
  });

  it("New project picks a folder, initializes it and opens it as a project", async () => {
    await store.start().catch(() => {});
    started = true;
    await toWelcome();
    await until(() => store.view.state === "welcome", "welcome");
    component = mount(Welcome, { target, props: { store } });
    flushSync();
    byText("button", "New project").click();
    // the demo folder has files in it: asked first
    await until(() => ui.initDialog.status === "asking", "asked");
    expect(store.view.state).toBe("welcome");
    await confirmInit(MOCK_FOLDER, true);
    await until(() => store.view.state === "showing", "opened");
    expect(mock.probe.shell.project().initialized).toBe(true);
    await until(() => conditions.current.permark === "initialized", "initialized");
  });
});
