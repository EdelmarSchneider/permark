// @vitest-environment jsdom
// The Panels button of the status bar: a menu button (its state said), the menu of the views by keyboard, Escape
// and a pick give the keyboard back to the button; the palette opens the same menu with the keyboard in it.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { api } = await import("../../api");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { en } = await import("../i18n/table");
const { contributions } = await import("../plugins/instance.svelte");
const { windowStore } = await import("../window.svelte");
const { layout } = await import("./layout.svelte");
const { arrangement } = await import("./arrangement.svelte");
const { ui } = await import("./ui.svelte");
const { default: PanelsButton } = await import("./PanelsButton.svelte");

const TAGS = "tags.panel";
let component: Record<string, unknown> | null = null;
const removals: (() => void)[] = [];

beforeEach(async () => {
  conditions.set({ ...neutralContext(), workspace: "project" });
  await api.projectSessionSet("layout.views", null);
  await layout.load(api, true);
  removals.push(
    contributions.register({
      kind: "panel",
      id: TAGS,
      owner: "tags" as PluginId,
      side: "left",
      label: "tags.title",
      requires: NO_REQUIREMENT,
      view: () => import("../testing/StripFirst.svelte"),
    }),
  );
  component = mount(PanelsButton, { target: document.body });
  flushSync();
});

afterEach(async () => {
  if (component) void unmount(component);
  component = null;
  ui.closePanels();
  for (const notice of windowStore.notices) windowStore.dismiss(notice.id);
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  await layout.load(api, false);
  document.body.replaceChildren();
});

const button = (): HTMLButtonElement => {
  const found = document.querySelector<HTMLButtonElement>("button[aria-haspopup]");
  if (found === null) throw new Error("no Panels button");
  return found;
};
const menuEl = (): HTMLElement | null => document.querySelector<HTMLElement>('[role="menu"]');
const key = (k: string): void => {
  document.activeElement?.dispatchEvent(
    new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }),
  );
  flushSync();
};
const focusedText = (): string =>
  document.activeElement instanceof HTMLElement ? document.activeElement.textContent.trim() : "";

describe("the Panels button", () => {
  it("is a menu button named Panels, its state said", () => {
    expect(button().getAttribute("aria-label")).toBe(en["panel.panels"]);
    expect(button().getAttribute("aria-haspopup")).toBe("menu");
    expect(button().getAttribute("aria-expanded")).toBe("false");
    expect(button().title).toBe(en["panel.panelsHint"]);
    expect(menuEl()).toBeNull();
  });

  it("opens the menu of the views, the keyboard on its first entry that can act; the arrows and Home | End move it", () => {
    button().focus();
    button().click();
    flushSync();
    expect(button().getAttribute("aria-expanded")).toBe("true");
    expect(menuEl()?.getAttribute("aria-label")).toBe(en["panel.panels"]);
    expect(document.activeElement?.getAttribute("role")).toBe("menuitemcheckbox");
    // Files and Search are always open: the keyboard starts after them
    expect(focusedText()).toBe("Tags");
    expect(document.activeElement?.getAttribute("aria-checked")).toBe("true");
    key("ArrowUp");
    expect(focusedText()).toBe("Search");
    key("End");
    expect(focusedText()).toBe(en["panel.resetLayout"]);
    key("Home");
    expect(focusedText()).toBe("Files");
  });

  it("Escape closes it and gives the keyboard back to the button", async () => {
    button().focus();
    button().click();
    flushSync();
    key("Escape");
    await tick();
    expect(menuEl()).toBeNull();
    expect(button().getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(button());
  });

  it("a pick switches the view and gives the keyboard back to the button", async () => {
    button().focus();
    button().click();
    flushSync();
    key("Enter");
    await tick();
    expect(menuEl()).toBeNull();
    expect(arrangement.isPresent(TAGS)).toBe(false);
    expect(document.activeElement).toBe(button());
  });

  it("a click on a view switches it and keeps the menu open for the next one, the keyboard in it", () => {
    button().click();
    flushSync();
    const tags = [...document.querySelectorAll<HTMLElement>('[role="menuitemcheckbox"]')].find(
      (el) => el.textContent.trim() === "Tags",
    );
    if (tags === undefined) throw new Error("no Tags entry");
    tags.click();
    flushSync();
    expect(arrangement.isPresent(TAGS)).toBe(false);
    expect(menuEl()).not.toBeNull();
    expect(ui.panels).toBe(true);
    expect(tags.getAttribute("aria-checked")).toBe("false");
    expect(menuEl()?.contains(document.activeElement)).toBe(true);
    tags.click();
    flushSync();
    expect(arrangement.isPresent(TAGS)).toBe(true);
    expect(tags.getAttribute("aria-checked")).toBe("true");
  });

  it("pressed again, it closes the menu", () => {
    button().click();
    flushSync();
    button().click();
    flushSync();
    expect(menuEl()).toBeNull();
  });

  it("gone while its menu is open (the window left the project): the menu does not come back with the next one", () => {
    button().click();
    flushSync();
    if (component) void unmount(component);
    component = null;
    flushSync();
    expect(ui.panels).toBe(false);
  });

  it("opened from the palette: the keyboard in the menu; Escape puts it on the button", async () => {
    ui.openPanels();
    flushSync();
    expect(menuEl()).not.toBeNull();
    expect(menuEl()?.contains(document.activeElement)).toBe(true);
    key("Escape");
    await tick();
    expect(document.activeElement).toBe(button());
  });
});
