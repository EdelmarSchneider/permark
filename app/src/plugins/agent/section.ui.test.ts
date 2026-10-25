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
    () => contributions.visible("settingsPage", conditions.current).length === 1,
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
  await store.changeSetting({ key: "pluginEnabled", value: { plugin: "agent", enabled: null } });
});

describe("Settings > AI agents", () => {
  it("has its own place in the nav, after Editing; Integration has no agent card", async () => {
    await open();
    mountPage("reading");
    await until(() => navLabels().includes("AI agents"), "nav item");
    const labels = navLabels();
    expect(labels.indexOf("AI agents")).toBe(labels.indexOf("Editing") + 1);
    expect(labels.indexOf("Integration")).toBe(labels.indexOf("AI agents") + 1);
    ui.showSettingsSection("integration");
    flushSync();
    await until(() => target.querySelectorAll("section.group").length >= 2, "integration cards");
    const headings = Array.from(target.querySelectorAll("section.group h3")).map(text);
    expect(headings).not.toContain("Agent connection");
    expect(text(target)).not.toContain("Share this project with agents");
  });

  it("off: says what is shared and offers one button; turning on shows the steps and moves focus to Copy", async () => {
    await open();
    mountPage(AGENT_PAGE, true);
    await until(() => text(target).includes("Off for this project"), "state card");
    expect(text(target.querySelector(".title"))).toBe("AI agents");
    expect(text(target)).toContain("agents never edit files through PerMark.");
    expect(text(target)).toContain("an agent can still open files on your computer by itself.");
    expect(text(target)).toContain("You can turn it off at any time.");
    const primary = button("Share this project with agents");
    expect(primary.classList.contains("btn--primary")).toBe(true);
    // opened from "Set up…": focus on the button; nothing is on yet
    await until(() => document.activeElement === primary, "focus on the primary button");
    expect(agent.enabled).toBe(false);
    // no setup lines while off
    expect(text(target)).not.toContain("claude mcp add");
    primary.click();
    await until(() => text(target).includes("On · waiting for an agent"), "on");
    expect(text(target)).toContain("Connect your agent");
    expect(text(target)).toContain("Claude Code: run this once in the project folder");
    await until(() => text(target).includes("claude mcp add permark --"), "the command");
    const copy = button("Copy");
    await until(() => document.activeElement === copy, "focus on the first Copy");
    expect(text(target.querySelector('[role="status"]'))).toBe(
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
    // turn off: focus back on the primary button
    button("Turn off").click();
    await until(() => text(target).includes("Off for this project"), "off again");
    expect(text(target)).not.toContain("Connect your agent");
    await until(
      () => document.activeElement === button("Share this project with agents"),
      "focus back on the primary button",
    );
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
    await until(() => text(target).includes("Off for this project"), "off");
    const notices = windowStore.notices.length;
    const call = vi.spyOn(mock, "pluginCall").mockResolvedValueOnce({
      ok: false,
      error: {
        kind: "backend",
        command: "plugin_call",
        error: { kind: "io", subject: null, detail: null },
      },
    });
    button("Share this project with agents").click();
    await until(() => target.querySelector('[role="alert"]') !== null, "the inline refusal");
    expect(text(target.querySelector('[role="alert"]'))).toBe(
      "The change wasn't saved. Try again.",
    );
    expect(text(target)).toContain("Off for this project");
    expect(windowStore.notices.length).toBe(notices);
    call.mockRestore();
  });

  it("what agents may do is folded, with its count", async () => {
    await open();
    mountPage(AGENT_PAGE);
    const fold = await vi.waitFor(() => button("What agents may do"));
    expect(text(fold)).toBe("What agents may do (7 of 8 allowed)");
    expect(fold.getAttribute("aria-expanded")).toBe("false");
    expect(target.querySelectorAll("input[type=checkbox]")).toHaveLength(0);
    fold.click();
    flushSync();
    expect(fold.getAttribute("aria-expanded")).toBe("true");
    expect(target.querySelectorAll("input[type=checkbox]")).toHaveLength(8);
    expect(text(target)).toContain("Use these for new projects");
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
