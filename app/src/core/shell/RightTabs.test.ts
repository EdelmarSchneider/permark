// @vitest-environment jsdom
// The right panel's tab strip: drawn only with two views or more, the dormant panels left out, the count on a tab,
// one Tab stop with arrows, selecting never hides (shell.md §Frame, Right region).
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { NO_REQUIREMENT } = await import("../conditions/available");
const { contributions } = await import("../plugins/instance.svelte");
const { reading } = await import("../doc/reading.svelte");
const { layout } = await import("./layout.svelte");
const { ui } = await import("./ui.svelte");
const RightTabs = (await import("./RightTabs.svelte")).default;

const removals: (() => void)[] = [];
let component: Record<string, unknown> | null = null;
let offered = true;
let badge: string | null = null;

function panel(id: string, owner = "live-feed"): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id,
      owner: owner as PluginId,
      side: "right",
      label: "live.label",
      requires: NO_REQUIREMENT,
      view: () => import("../testing/StripFirst.svelte"),
      offered: () => offered,
      badge: () => badge,
    }),
  );
}

function show(outline: boolean): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(RightTabs, { target, props: { outline } });
  flushSync();
  return target;
}

const tabs = (root: HTMLElement): HTMLButtonElement[] => [
  ...root.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
];

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  for (const remove of removals.splice(0)) remove();
  offered = true;
  badge = null;
  ui.right = null;
  layout.setRightHidden(false);
  reading.setOutline(true);
});

describe("the right panel's tabs", () => {
  it("are not drawn with one view, nor while the panel is dormant", () => {
    expect(show(true).querySelector('[role="tablist"]')).toBeNull();
    document.body.replaceChildren();
    offered = false;
    panel("live-feed.panel");
    expect(show(true).querySelector('[role="tablist"]')).toBeNull();
  });

  it("list Outline + the offered panel, the outline selected, the count on the panel's tab", () => {
    badge = "3";
    panel("live-feed.panel");
    const root = show(true);
    const [outline, live] = tabs(root);
    expect(root.querySelector('[role="tablist"]')?.getAttribute("aria-label")).toBe(
      "Right panel views",
    );
    expect(outline?.textContent?.trim()).toBe("Outline");
    expect(outline?.getAttribute("aria-selected")).toBe("true");
    expect(live?.getAttribute("aria-selected")).toBe("false");
    expect(live?.getAttribute("aria-label")).toBe("Live changes (3)");
    expect(live?.querySelector(".badge")?.textContent).toBe("3");
    // one Tab stop: the selected tab
    expect(outline?.tabIndex).toBe(0);
    expect(live?.tabIndex).toBe(-1);
  });

  it("select a view on click, never hide the panel, and the outline comes back", () => {
    panel("live-feed.panel");
    const root = show(true);
    tabs(root)[1]?.click();
    flushSync();
    expect(ui.right).toBe("live-feed.panel");
    // clicking the selected tab again changes nothing
    tabs(root)[1]?.click();
    flushSync();
    expect(ui.right).toBe("live-feed.panel");
    expect(layout.rightHidden).toBe(false);
    tabs(root)[0]?.click();
    flushSync();
    expect(ui.right).toBeNull();
    expect(reading.outline).toBe(true);
  });

  it("the outline tab turns the outline on when it was off", () => {
    panel("live-feed.panel");
    ui.right = "live-feed.panel";
    reading.setOutline(false);
    const root = show(true);
    tabs(root)[0]?.click();
    flushSync();
    expect(ui.right).toBeNull();
    expect(reading.outline).toBe(true);
  });

  it("move the focus with arrows, Home and End, without selecting", () => {
    panel("live-feed.panel");
    const root = show(true);
    const strip = root.querySelector<HTMLElement>('[role="tablist"]');
    tabs(root)[0]?.focus();
    strip?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    flushSync();
    expect(document.activeElement).toBe(tabs(root)[1]);
    expect(tabs(root)[1]?.tabIndex).toBe(0);
    expect(ui.right).toBeNull();
    strip?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    flushSync();
    expect(document.activeElement).toBe(tabs(root)[0]);
    strip?.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    flushSync();
    expect(document.activeElement).toBe(tabs(root)[1]);
    strip?.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true }));
    flushSync();
    expect(document.activeElement).toBe(tabs(root)[0]);
  });

  it("without an outline, two panels still make a strip", () => {
    panel("live-feed.panel");
    panel("other.panel", "comments");
    const root = show(false);
    expect(tabs(root)).toHaveLength(2);
    expect(tabs(root)[0]?.getAttribute("aria-selected")).toBe("false");
    // nothing selected: the first tab keeps the Tab stop
    expect(tabs(root)[0]?.tabIndex).toBe(0);
  });
});
