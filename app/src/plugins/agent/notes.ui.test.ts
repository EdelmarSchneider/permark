// @vitest-environment jsdom
// The notes shared with a project, in the status popover and in Settings > AI agents, against the mock backend: the
// list, the inline picker (keyboard, search, Escape closes the picker before the popover), stopping, the empty and
// the no-notes texts, and nothing at all while the notes plugin is off.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { default: SettingsPage } = await import("../../core/shell/settings/SettingsPage.svelte");
const { ui } = await import("../../core/shell/ui.svelte");
const { windowStore } = await import("../../core/window.svelte");
const { contributions } = await import("../../core/plugins/instance.svelte");
const { conditions } = await import("../../core/conditions/context.svelte");
const { agent } = await import("./store.svelte");
const { AGENT_PAGE } = await import("./offer.svelte");
const { default: AgentStatus } = await import("./AgentStatus.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const READING = "2026-10-16 1031.md";
const TRIP = "2026-10-19 0907.md";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function text(el: Element | null | undefined): string {
  return el?.textContent?.replace(/\s+/g, " ").trim() ?? "";
}

async function open(): Promise<void> {
  if (!started) {
    await windowStore.start();
    started = true;
  }
  await mock.openPath(MOCK_FOLDER, "thisTab");
  await until(
    () => windowStore.view.state === "showing" && windowStore.view.workspace.kind === "project",
    "the project shows",
  );
  await until(
    () =>
      contributions.visible("settingsPage", conditions.current).some((p) => p.id === AGENT_PAGE),
    "the agent section is contributed",
  );
  await until(() => agent.status !== null, "the agent status is known");
  await agent.setEnabled(true);
}

async function openPopover(): Promise<void> {
  component = mount(AgentStatus, { target });
  await until(() => target.querySelector(".chip") !== null, "the chip");
  target.querySelector<HTMLButtonElement>(".chip")?.click();
  await until(() => target.querySelector(".popover .shared-notes") !== null, "the notes");
}

const rows = (): string[] =>
  Array.from(target.querySelectorAll(".shared-notes .note-title")).map(text);
const addButton = (): HTMLButtonElement | null =>
  target.querySelector<HTMLButtonElement>("button[data-add-note]");
const field = (): HTMLInputElement | null =>
  target.querySelector<HTMLInputElement>(".shared-notes input[type=search]");
const backButton = (): HTMLButtonElement | null =>
  target.querySelector<HTMLButtonElement>("button[data-back]");
const options = (): string[] => Array.from(target.querySelectorAll('[role="option"]')).map(text);
const said = (): string => text(target.querySelector('.shared-notes [role="status"]'));

function key(el: Element | null, name: string, init: KeyboardEventInit = {}): void {
  el?.dispatchEvent(
    new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...init }),
  );
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
  // back to the seed: only the reading list shared
  const now = await mock.pluginCall("agent.sharedNotes", {});
  for (const note of now.ok ? now.value.shared : []) {
    await mock.pluginCall("agent.unshareNote", { name: note.id });
  }
  await mock.pluginCall("agent.shareNote", { name: READING });
  await mock.settingsSet({ key: "pluginEnabled", value: { plugin: "notes", enabled: null } });
  if (agent.enabled) await agent.setEnabled(false);
});

describe("notes shared with the project, in the popover", () => {
  it("lists them and adds from the inline picker with the keyboard and the pointer; it stays open", async () => {
    await open();
    await openPopover();
    expect(text(target.querySelector(".popover"))).toContain("Notes shared with this project");
    await until(() => rows().length === 1, "one shared note");
    expect(rows()).toEqual(["Reading list"]);
    expect(target.querySelector(".note-title")?.getAttribute("title")).toBe("Reading list");
    expect(
      target.querySelector('button[aria-label="Stop sharing Reading list"]')?.getAttribute("title"),
    ).toBe("Stop sharing");
    addButton()?.click();
    flushSync();
    await until(() => document.activeElement === field(), "focus in the search field");
    expect(field()?.placeholder).toBe("Search notes");
    // the newest first, the shared one not offered
    expect(options()).toEqual([
      "Trip to São Paulo",
      "Call the bank about the card",
      "Server IPs",
      "Recipe: lentil soup",
    ]);
    const listbox = target.querySelector('[role="listbox"]');
    const firstId = target.querySelector('[role="option"]')?.id ?? "";
    expect(field()?.getAttribute("aria-activedescendant")).toBe(firstId);
    // a key of an input method composing goes to it
    key(field(), "ArrowDown", { isComposing: true });
    expect(field()?.getAttribute("aria-activedescendant")).toBe(firstId);
    key(field(), "ArrowDown");
    expect(listbox?.querySelector('[aria-selected="true"]')?.textContent).toContain(
      "Call the bank",
    );
    key(field(), "Enter");
    await until(() => rows().length === 2, "shared by Enter");
    expect(rows()).toContain("Call the bank about the card");
    expect(said()).toBe("“Call the bank about the card” is now shared with this project's agents.");
    // still open, on the next candidate
    expect(field()).not.toBeNull();
    expect(options()).not.toContain("Call the bank about the card");
    // search
    const input = field()!;
    input.value = "lentil";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(options()).toEqual(["Recipe: lentil soup"]);
    // a pointer press on an option leaves the keyboard in the search field
    const press = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    target.querySelector('[role="option"]')?.dispatchEvent(press);
    expect(press.defaultPrevented).toBe(true);
    target.querySelector<HTMLElement>('[role="option"]')?.click();
    await until(() => rows().length === 3, "shared by a click");
    expect(document.activeElement).toBe(field());
    // Escape closes the picker only; focus back on Add note; the next Escape closes the popover
    key(field(), "Escape");
    expect(field()).toBeNull();
    expect(target.querySelector(".popover")).not.toBeNull();
    await until(() => document.activeElement === addButton(), "focus back on Add note");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    flushSync();
    expect(target.querySelector(".popover")).toBeNull();
  });

  it("Back closes the picker; with no notes at all the picker says so", async () => {
    await open();
    await openPopover();
    await until(() => rows().length === 1, "one shared note");
    addButton()?.click();
    flushSync();
    target.querySelector<HTMLButtonElement>("button[data-back]")?.click();
    flushSync();
    expect(field()).toBeNull();
    // one note left to add (the others put back at the end: the mock outlives the test)
    const held = new Map(mock.probe.plugins.notes.notes);
    for (const name of held.keys()) {
      if (name !== READING && name !== TRIP) mock.probe.plugins.notes.notes.delete(name);
    }
    await agent.refreshNotes();
    addButton()?.click();
    flushSync();
    await until(() => document.activeElement === field(), "focus in the search field");
    expect(options()).toEqual(["Trip to São Paulo"]);
    // the last one added: the field goes, the keyboard lands on Back, never on the page
    key(field(), "Enter");
    await until(() => rows().length === 2, "shared by Enter");
    expect(text(target.querySelector(".shared-notes"))).toContain("You have no notes yet.");
    await until(() => document.activeElement === backButton(), "focus on Back");
    // opened with nothing to add: the keyboard starts on Back
    backButton()?.click();
    flushSync();
    addButton()?.click();
    flushSync();
    expect(options()).toEqual([]);
    await until(() => document.activeElement === backButton(), "focus on Back when empty");
    for (const [name, note] of held) mock.probe.plugins.notes.notes.set(name, note);
  });

  it("stops sharing with the X: the row goes, it is said, focus stays in the list; then the empty text", async () => {
    await open();
    await agent.shareNote("2026-10-19 0907.md");
    await openPopover();
    await until(() => rows().length === 2, "two shared notes");
    target
      .querySelector<HTMLButtonElement>('button[aria-label="Stop sharing Trip to São Paulo"]')
      ?.click();
    await until(() => rows().length === 1, "one left");
    expect(said()).toBe("“Trip to São Paulo” is no longer shared.");
    await until(
      () =>
        document.activeElement ===
        target.querySelector('button[aria-label="Stop sharing Reading list"]'),
      "focus on the next row",
    );
    target
      .querySelector<HTMLButtonElement>('button[aria-label="Stop sharing Reading list"]')
      ?.click();
    await until(() => rows().length === 0, "none left");
    expect(text(target.querySelector(".shared-notes"))).toContain(
      "No notes shared. Agents here can read only the notes you add.",
    );
    await until(() => document.activeElement === addButton(), "focus on Add note");
  });

  it("opens with the keyboard on the popover itself, never on a Disconnect", async () => {
    await open();
    mock.probe.plugins.agent.connect("Claude Code");
    await until(() => agent.sessions.length === 1, "one agent connected");
    await openPopover();
    const popover = target.querySelector(".popover");
    await until(() => document.activeElement === popover, "focus on the popover");
    expect(target.querySelector("button[data-disconnect]")).not.toBeNull();
    mock.probe.plugins.agent.connect(null);
  });

  it("draws nothing while the notes plugin is off", async () => {
    await open();
    await mock.settingsSet({ key: "pluginEnabled", value: { plugin: "notes", enabled: false } });
    component = mount(AgentStatus, { target });
    await until(() => target.querySelector(".chip") !== null, "the chip");
    target.querySelector<HTMLButtonElement>(".chip")?.click();
    await until(() => target.querySelector(".popover") !== null, "the popover");
    await agent.refreshNotes();
    flushSync();
    expect(target.querySelector(".shared-notes")).toBeNull();
    expect(text(target.querySelector(".popover"))).not.toContain("Notes shared with this project");
  });
});

describe("notes shared with the project, in Settings > AI agents", () => {
  it("is a card under the agents while on, and goes with the notes plugin", async () => {
    await open();
    ui.openSettingsPage(AGENT_PAGE, "nav");
    component = mount(SettingsPage, { target, props: { store: windowStore } });
    await until(() => rows().length === 1, "the card lists the shared note");
    const headings = Array.from(target.querySelectorAll("h4.group-title")).map(text);
    expect(headings.indexOf("Notes shared with this project")).toBe(
      headings.indexOf("Connected agents") + 1,
    );
    await mock.settingsSet({ key: "pluginEnabled", value: { plugin: "notes", enabled: false } });
    await agent.refreshNotes();
    await until(() => target.querySelector(".shared-notes") === null, "the card goes");
    expect(text(target)).not.toContain("Notes shared with this project");
  });
});
