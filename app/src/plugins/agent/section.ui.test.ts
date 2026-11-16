// @vitest-environment jsdom
// Settings > AI agents against the mock backend: its place in the nav, the state card and its one button, the setup
// steps while on, the folded permissions, the gear popover row, focus moves, and nothing left with the plugin off.
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { default: SettingsPage } = await import("../../core/shell/settings/SettingsPage.svelte");
const { default: SettingsPopover } = await import("../../core/shell/SettingsPopover.svelte");
const { ui } = await import("../../core/shell/ui.svelte");
const { windowStore } = await import("../../core/window.svelte");
const { contributions } = await import("../../core/plugins/instance.svelte");
const { conditions } = await import("../../core/conditions/context.svelte");
const { agent } = await import("./store.svelte");
const { AGENT_PAGE } = await import("./offer.svelte");
const { default: AgentStatus } = await import("./AgentStatus.svelte");
const { t } = await import("../../core/i18n");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const store = windowStore;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
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
  await until(
    () =>
      contributions.visible("settingsPage", conditions.current).some((p) => p.id === AGENT_PAGE),
    "the agent section is contributed",
  );
  await until(() => agent.status !== null, "the agent status is known");
}

function text(el: Element | null | undefined): string {
  return el?.textContent?.replace(/\s+/g, " ").trim() ?? "";
}

function button(label: string): HTMLButtonElement {
  const found = Array.from(target.querySelectorAll<HTMLButtonElement>("button")).find(
    (b) => text(b) === label || text(b).startsWith(label),
  );
  if (!found) throw new Error(`no button "${label}"`);
  return found;
}

/** The person's switch for this folder (named by its label). */
function shareSwitch(): HTMLInputElement {
  const found = Array.from(target.querySelectorAll<HTMLInputElement>('input[role="switch"]')).find(
    (box) => text(box.closest("label")) === "Share this folder with agents",
  );
  if (!found) throw new Error("no switch");
  return found;
}

function navLabels(): string[] {
  return Array.from(target.querySelectorAll(".nav-item")).map(text);
}

function mountPage(section: Parameters<typeof ui.openSettingsPage>[0], focus = false): void {
  ui.openSettingsPage(section, focus ? "content" : "nav");
  component = mount(SettingsPage, { target, props: { store } });
  flushSync();
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  ui.closeSettingsPage();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  if (agent.enabled) await agent.setEnabled(false);
  mock.probe.plugins.agent.fail(null);
  mock.probe.plugins.agent.connect(null);
  mock.probe.plugins.agent.refused.clear();
  await store.changeSetting({ key: "pluginEnabled", value: { plugin: "agent", enabled: null } });
});

describe("Settings > AI agents", () => {
  it("has its own place in the This folder group of the nav; Integration has no agent card", async () => {
    await open();
    mountPage("reading");
    // plugins come on one by one, each when its module is loaded: wait for every page whose place is checked
    await until(
      () => ["AI agents", "Local AI"].every((label) => navLabels().includes(label)),
      "nav items",
    );
    const group = (name: string): string[] =>
      Array.from(
        target.querySelectorAll(`.nav [role="group"][aria-label="${name}"] .nav-item`),
      ).map(text);
    // the agents are a matter of this folder, after its overview and files
    const folder = group("This folder");
    expect(folder.indexOf("AI agents")).toBe(folder.indexOf("Files") + 1);
    // Local AI (order 35) is the app's: between Editing and Integration
    const app = group("App");
    expect(app).not.toContain("AI agents");
    expect(app.indexOf("Local AI")).toBe(app.indexOf("Editing") + 1);
    expect(app.indexOf("Integration")).toBe(app.indexOf("Local AI") + 1);
    ui.showSettingsSection("integration");
    flushSync();
    await until(() => target.querySelectorAll("section.group").length >= 2, "integration cards");
    const headings = Array.from(target.querySelectorAll("section.group h4")).map(text);
    expect(headings).not.toContain("Agent connection");
    expect(text(target)).not.toContain("Share this folder with agents");
  });

  it("off: says what is shared and offers one switch; turning it on shows the steps, the focus stays on it", async () => {
    await open();
    mountPage(AGENT_PAGE, true);
    await until(() => text(target).includes("Off for this folder"), "state card");
    expect(text(target.querySelector(".title"))).toBe("AI agents");
    expect(text(target)).toContain("agents never edit files through PerMark.");
    expect(text(target)).toContain("an agent can still open files on your computer by itself.");
    expect(text(target)).toContain("search and read this folder's files");
    expect(text(target)).toContain("You can turn it off at any time.");
    const toggle = shareSwitch();
    expect(toggle.getAttribute("role")).toBe("switch");
    expect(toggle.checked).toBe(false);
    // opened from "Set up…": focus on the switch; nothing is on yet
    await until(() => document.activeElement === toggle, "focus on the switch");
    expect(agent.enabled).toBe(false);
    // no setup lines while off
    expect(text(target)).not.toContain("claude mcp add");
    toggle.click();
    await until(() => text(target).includes("On · waiting for an agent"), "on");
    expect(text(target)).toContain("Connect your agent");
    expect(text(target)).toContain("Claude Code: run this once in the project folder");
    await until(() => text(target).includes("claude mcp add permark --"), "the command");
    // the same switch, now on, keeps the keyboard (the steps below are announced, not jumped to)
    expect(shareSwitch()).toBe(toggle);
    expect(toggle.checked).toBe(true);
    await tick();
    expect(document.activeElement).toBe(toggle);
    expect(Array.from(target.querySelectorAll('[role="status"]')).map(text)).toContain(
      "Sharing is on. Connect your agent with the steps below.",
    );
    // more ways, folded
    const more = button("More ways to connect");
    expect(more.getAttribute("aria-expanded")).toBe("false");
    expect(text(target)).not.toContain("mcpServers");
    more.click();
    flushSync();
    expect(more.getAttribute("aria-expanded")).toBe("true");
    expect(text(target)).toContain("mcpServers");
    expect(text(target)).toContain("UserPromptSubmit");
    // turn off with the same switch: the focus stays on it
    toggle.click();
    await until(() => text(target).includes("Off for this folder"), "off again");
    expect(text(target)).not.toContain("Connect your agent");
    expect(toggle.checked).toBe(false);
    await tick();
    expect(document.activeElement).toBe(toggle);
  });

  it("a folder created again at its path shows off, says why, and goes back on with the one button", async () => {
    await open();
    mountPage(AGENT_PAGE);
    await until(() => text(target).includes("Off for this folder"), "state card");
    expect(text(target)).not.toContain("This folder changed since you connected it.");
    mock.probe.plugins.agent.replaceFolder();
    await until(
      () =>
        text(target).includes(
          "This folder changed since you connected it. Turn the connection on again to allow agents here.",
        ),
      "the note",
    );
    expect(text(target)).toContain("Off for this folder");
    shareSwitch().click();
    await until(() => text(target).includes("On · waiting for an agent"), "on");
    expect(text(target)).not.toContain("This folder changed since you connected it.");
  });

  it("names the connected client, and a server error with its reason and a mark (not color only)", async () => {
    await open();
    mountPage(AGENT_PAGE);
    await agent.setEnabled(true);
    mock.probe.plugins.agent.connect("Claude Code");
    await until(() => text(target).includes("On · connected: Claude Code"), "connected");
    mock.probe.plugins.agent.fail("inUse");
    await until(() => text(target).includes("On · the connection couldn't start"), "error");
    expect(text(target)).toContain(
      "Another PerMark already serves this project, so this window can't.",
    );
    expect(target.querySelector(".state svg")).not.toBeNull();
    expect(target.querySelector(".state .dot")).toBeNull();
  });

  it("a refused switch keeps the state and says so in place", async () => {
    await open();
    mountPage(AGENT_PAGE);
    await until(() => text(target).includes("Off for this folder"), "off");
    const notices = windowStore.notices.length;
    const call = vi.spyOn(mock, "pluginCall").mockResolvedValueOnce({
      ok: false,
      error: {
        kind: "backend",
        command: "plugin_call",
        error: { kind: "io", subject: null, detail: null },
      },
    });
    shareSwitch().click();
    await until(() => target.querySelector('[role="alert"]') !== null, "the inline refusal");
    expect(text(target.querySelector('[role="alert"]'))).toBe(
      "The change wasn't saved. Try again.",
    );
    expect(text(target)).toContain("Off for this folder");
    expect(shareSwitch().checked).toBe(false);
    expect(windowStore.notices.length).toBe(notices);
    call.mockRestore();
  });

  it("what agents may do is folded, with its count", async () => {
    await open();
    mountPage(AGENT_PAGE);
    const fold = await vi.waitFor(() => button("What agents may do"));
    expect(text(fold)).toBe("What agents may do (7 of 8 allowed)");
    expect(fold.getAttribute("aria-expanded")).toBe("false");
    // the person's own groups (the folder's limits below have boxes of their own)
    const own = (): number =>
      Array.from(target.querySelectorAll("input[type=checkbox]:not([role=switch])")).filter(
        (box) => box.closest(".limits-rows") === null,
      ).length;
    expect(own()).toBe(0);
    fold.click();
    flushSync();
    expect(fold.getAttribute("aria-expanded")).toBe("true");
    expect(own()).toBe(8);
    expect(text(target)).toContain("Use these for new projects");
  });

  it("holds the limits for everyone below the person's own switch; no way out to another section", async () => {
    await open();
    mountPage(AGENT_PAGE);
    await until(() => text(target).includes("Agent limits"), "the limits");
    // the switch is the person's own; the limits are the folder's, for everyone
    expect(target.querySelector(".state-card .scope-chip")?.getAttribute("data-scope")).toBe("you");
    // sharing off here: the limits fold into one line, still there to edit for everyone
    const fold = button("Agent limits");
    expect(fold.getAttribute("aria-expanded")).toBe("false");
    expect(target.querySelector(".limits-rows")).toBeNull();
    fold.click();
    flushSync();
    expect(fold.getAttribute("aria-expanded")).toBe("true");
    // ticked = blocked, said in the row itself
    expect(text(target)).toContain("Groups blocked for agents here");
    expect(text(target)).toContain("Tick a group to keep agents out of it, for everyone.");
    const limits = Array.from(target.querySelectorAll<HTMLElement>(".limits-rows .row"));
    expect(limits.map((r) => r.querySelector(".scope-chip")?.getAttribute("data-scope"))).toEqual([
      "everyone",
      "everyone",
    ]);
    expect(text(target)).not.toContain("This folder settings");
    expect(text(target)).toContain("Paths agents never get");
    expect(text(target)).toContain(
      "Files listed in .aiignore never reach agents. These patterns add to them",
    );
    const field = target.querySelector<HTMLInputElement>(".limits-rows .list input")!;
    expect(field.placeholder).toBe("Pattern, for example secrets/**");
    // no .permark/ yet: greyed, nothing written until the person sets PerMark up
    expect(text(target)).toContain("Initialize PerMark to share this with everyone");
    expect(field.disabled).toBe(true);
    const box = (): HTMLInputElement =>
      Array.from(target.querySelectorAll<HTMLLabelElement>(".limits-rows label.group"))
        .find((l) => text(l).includes("Read notes you share"))!
        .querySelector("input")!;
    expect(box().disabled).toBe(true);
    await mock.projectInit(MOCK_FOLDER);
    await until(() => !box().disabled, "the limits work");
    expect(text(target)).not.toContain("Initialize PerMark to share this with everyone");
    box().click();
    await until(() => mock.probe.shell.project().agentDeny.includes("notes"), "narrowed");
    // sharing on: the limits show open, under their heading (no fold)
    await agent.setEnabled(true);
    await until(() => !text(target).includes("Off for this folder"), "on");
    expect(target.querySelector("h4.limits-title")).not.toBeNull();
    expect(target.querySelector(".limits-rows")).not.toBeNull();
    expect(
      Array.from(target.querySelectorAll("button")).some((b) => text(b) === "Agent limits"),
    ).toBe(false);
    mock.probe.shell.forgetInit();
  });

  it("goes with the plugin: nav item gone, an open page on it falls back to Reading", async () => {
    await open();
    mountPage(AGENT_PAGE);
    await until(() => text(target.querySelector(".title")) === "AI agents", "shown");
    await store.changeSetting({ key: "pluginEnabled", value: { plugin: "agent", enabled: false } });
    await until(() => !navLabels().includes("AI agents"), "nav item gone");
    expect(text(target.querySelector(".title"))).toBe("Reading");
  });
});

describe("the gear popover row", () => {
  it("says the state in a word and opens the section on its first control", async () => {
    await open();
    component = mount(SettingsPopover, { target, props: { store, close: () => {} } });
    await until(() => text(target).includes("AI agents"), "row");
    expect(text(target)).toContain("Off");
    const setUp = button("Set up…");
    // the last row before the keyboard map and All settings
    const keyboard = button("Keyboard shortcuts");
    expect(setUp.compareDocumentPosition(keyboard)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    setUp.click();
    expect(ui.settingsPage).toEqual({ status: "open", section: AGENT_PAGE });
    await agent.setEnabled(true);
    await until(() => text(target).includes("Settings…"), "on: Settings…");
    expect(text(target)).toContain("On");
  });

  it("takes focus on open, keeps Tab inside, reaches Set up… in 3 Tabs, Esc gives focus back to the gear", async () => {
    await open();
    const gear = document.createElement("button");
    gear.setAttribute("data-settings-trigger", "");
    document.body.append(gear);
    const close = vi.fn();
    component = mount(SettingsPopover, { target, props: { store, close } });
    await until(() => text(target).includes("Set up…"), "row");
    const panel = target.querySelector<HTMLElement>('[role="dialog"]')!;
    const stops = Array.from(
      panel.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]',
      ),
    ).filter((el) => el.tabIndex >= 0);
    expect(document.activeElement).toBe(stops[0]);
    expect(stops.indexOf(button("Set up…"))).toBeLessThanOrEqual(3);
    // Tab on the last control wraps to the first; Shift+Tab on the first to the last
    stops.at(-1)?.focus();
    panel.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }),
    );
    expect(document.activeElement).toBe(stops[0]);
    panel.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true }),
    );
    expect(document.activeElement).toBe(stops.at(-1));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(close).toHaveBeenCalled();
    expect(document.activeElement).toBe(gear);
  });

  it("says when the connection couldn't start, not 'On'", async () => {
    await open();
    await agent.setEnabled(true);
    mock.probe.plugins.agent.fail("inUse");
    component = mount(SettingsPopover, { target, props: { store, close: () => {} } });
    await until(() => text(target).includes("Couldn't start"), "the failure");
    expect(text(target)).not.toMatch(/AI agents\s*On/);
  });

  it("is not there outside a project", async () => {
    await open();
    await mock.openPath(`${MOCK_FOLDER}/README.md`, "thisTab");
    await until(() => conditions.current.workspace === "singleFile", "a single file");
    component = mount(SettingsPopover, { target, props: { store, close: () => {} } });
    await tick();
    expect(text(target)).not.toContain("AI agents");
  });
});

describe("the connected agents", () => {
  const disconnectOf = (name: string): HTMLButtonElement => {
    const found = target.querySelector<HTMLButtonElement>(
      `button[aria-label="Disconnect ${name}"]`,
    );
    if (!found) throw new Error(`no Disconnect for ${name}`);
    return found;
  };

  it("lists each agent; Disconnect ends one, leaves an Undo notice and moves focus along the list", async () => {
    await open();
    mountPage(AGENT_PAGE);
    await agent.setEnabled(true);
    await until(() => text(target).includes(t("agent.sessions.empty")), "the empty list");
    const probe = mock.probe.plugins.agent;
    probe.connect("Claude Code", "sessionaaaa");
    probe.connect("Claude Code", "sessionbbbb");
    probe.connect("Codex", "sessioncccc");
    probe.request({
      session: "sessioncccc",
      tool: "search",
      subject: null,
      atMs: Date.now(),
      outcome: "ok",
    });
    await until(
      () => target.querySelectorAll("button[data-disconnect]").length === 3,
      "three rows",
    );
    const names = Array.from(target.querySelectorAll(".agents .name")).map(text);
    expect(names).toEqual(["Claude Code", "Claude Code · 2", "Codex"]);
    expect(text(target)).toContain("Started now · project root");
    expect(text(target)).toContain("Last request: search · now");
    expect(text(target)).toContain("No requests yet");
    expect(target.querySelector('[role="region"][aria-label="Connected agents"]')).not.toBeNull();
    // the second one goes: the next row's button takes the focus
    disconnectOf("Claude Code · 2").click();
    await until(() => document.activeElement === disconnectOf("Codex"), "focus on the next row");
    expect(probe.refused.has("sessionbbbb")).toBe(true);
    expect(agent.sessions.map((s) => s.id)).toEqual(["sessionaaaa", "sessioncccc"]);
    const notice = windowStore.notices.at(-1);
    expect(notice?.kind === "action" && notice.key).toBe("agent.disconnect.sessionbbbb");
    expect(notice?.kind === "action" && notice.text).toBe("Claude Code · 2 disconnected.");
    expect(text(target.querySelector('.agents [role="status"]'))).toBe(
      "Claude Code · 2 disconnected.",
    );
    // the last row goes: the previous one; then the empty text
    disconnectOf("Codex").click();
    await until(
      () => document.activeElement === disconnectOf("Claude Code"),
      "focus on the previous row",
    );
    disconnectOf("Claude Code").click();
    await until(
      () => text(document.activeElement) === t("agent.sessions.empty"),
      "focus on the empty text",
    );
    // Undo lifts the refusal: that agent can pair again
    if (notice?.kind === "action") await notice.actions[0]?.run();
    expect(probe.refused.has("sessionbbbb")).toBe(false);
    probe.connect("Claude Code", "sessionbbbb");
    await until(() => agent.sessions.length === 1, "paired again");
  });

  it("the status item says Agent, the name, or how many", async () => {
    await open();
    await agent.setEnabled(true);
    component = mount(AgentStatus, { target });
    const chip = (): HTMLButtonElement | null => target.querySelector(".chip");
    await until(() => text(chip()) === "Agent", "none");
    mock.probe.plugins.agent.connect("Claude Code", "sessionaaaa");
    await until(() => text(chip()) === "Claude Code", "one");
    mock.probe.plugins.agent.connect("Codex", "sessionbbbb");
    await until(() => text(chip()) === "2 agents", "two");
    expect(chip()?.title).toBe("2 agents connected");
    // the popover lists them and starts the keyboard on its first control
    chip()?.click();
    flushSync();
    await until(
      () => target.querySelectorAll(".popover button[data-disconnect]").length === 2,
      "listed",
    );
    expect(document.activeElement?.closest(".popover")).not.toBeNull();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    flushSync();
    expect(target.querySelector(".popover")).toBeNull();
    expect(document.activeElement).toBe(chip());
  });
});
