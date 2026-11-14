// @vitest-environment jsdom
// The whole window on the mock: the frame draws a region where a view is present, whatever the kind of the workspace.
// A project's default screen (sidebar tabs, one view shown), a view moved to the other region with what it holds, a
// single file, the Notes with their own list over the views arranged at the left. The views' options: where their
// button shows, a move by the menu, a window too short for the bottom region.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { DocumentView } from "../../api";
import { MOCK_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};
// jsdom has no layout: nothing changes size by itself; `resized` says an element did
const sizeWatchers: ResizeObserverCallback[] = [];
globalThis.ResizeObserver = class {
  constructor(heard: ResizeObserverCallback) {
    sizeWatchers.push(heard);
  }
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};
/** The element is `height` px high now: what watches its size hears it. */
function resized(el: Element, height: number): void {
  Object.defineProperty(el, "clientHeight", { value: height, configurable: true });
  for (const heard of sizeWatchers) {
    heard([{ target: el } as ResizeObserverEntry], {} as ResizeObserver);
  }
  flushSync();
}

const { api } = await import("../../api");
const { windowStore: store } = await import("../window.svelte");
const { default: App } = await import("../../App.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { runnable } = await import("../commands/commands");
const { conditions } = await import("../conditions/context.svelte");
const { documents } = await import("../doc/document.svelte");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { BOTTOM, PAGE_MIN_H } = await import("./layout");
const { layout } = await import("./layout.svelte");
const { viewMenu } = await import("./viewMenu.svelte");
const { arrangement } = await import("./arrangement.svelte");
const { defaultArrangement, FILES, moveTo, OUTLINE, SEARCH } = await import("./arrangement");
const { showView } = await import("./sides");
const { ui } = await import("./ui.svelte");

let app: Record<string, unknown> | null = null;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

const region = (name: string): HTMLElement | null =>
  document.querySelector<HTMLElement>(`[data-region="${name}"]`);
const slot = (id: string): HTMLElement | null =>
  document.querySelector<HTMLElement>(`[data-view-slot="${id}"]`);
const regionAt = (el: Element | null): string | null =>
  el?.closest("[data-region]")?.getAttribute("data-region") ?? null;
const sidebarToggle = (): HTMLElement | null =>
  document.querySelector<HTMLElement>('footer button.toggle[aria-label="Sidebar"]');
const bottomToggle = (): HTMLElement | null =>
  document.querySelector<HTMLElement>('footer button.toggle[aria-label="Bottom panel"]');
const bottomBar = (): HTMLElement | null =>
  document.querySelector<HTMLElement>('[role="separator"][data-side="bottom"]');
const commandOf = (id: string) => {
  const command = contributions.of("command").find((c) => c.id === id);
  if (!command) throw new Error("no command");
  return command;
};
/** The options button at the end of a region's tab strips (not the one a view draws in its own header). */
const stripOptions = (name: string): HTMLElement[] =>
  [...(region(name)?.querySelectorAll<HTMLElement>("[data-view-menu]") ?? [])].filter(
    (button) => button.closest("[data-view-host]") === null,
  );
/** The options button a view draws in its own header. */
const headerOptions = (id: string): HTMLElement | null =>
  slot(id)?.querySelector<HTMLElement>(`[data-view-host] [data-view-menu="${id}"]`) ?? null;
const menuItems = (): HTMLElement[] => [
  ...document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]'),
];
/** Picks an entry of the open options menu; what it did is drawn and the keyboard has settled. */
async function pick(label: string): Promise<void> {
  const item = menuItems().find((el) => el.textContent?.trim() === label);
  if (item === undefined) throw new Error(`no entry "${label}"`);
  item.click();
  flushSync();
  await tick();
  await tick();
}
const toggleCommand = () => commandOf("sidebar.toggle");
const bottomCommand = () => commandOf("panel.toggleBottom");

/** A document with a heading is on screen (the outline has something to list). */
function showOutline(): void {
  const shown: DocumentView = {
    path: "a.md",
    name: "a.md",
    format: "markdown",
    role: null,
    body: {
      kind: "markdown",
      blocks: [],
      outline: [{ label: "Title", level: 1, id: "title", line: 1, block: "b1" }],
      frontMatter: null,
    },
    hash: "ab".repeat(8),
    size: 8,
    mtimeMs: 1,
    editable: { editable: "surgical" },
    notes: [],
  };
  documents.state = { status: "ready", view: shown, deleted: null, fresh: true };
  flushSync();
}

/** The outline arranged at the left, alone; the rest as by default. */
function outlineAtLeft(): void {
  arrangement.arrange(moveTo(defaultArrangement(), OUTLINE, "left"));
  flushSync();
}

beforeAll(async () => {
  await store.start();
  await api.settingsSet({ key: "openIn", value: "this" });
  const target = document.createElement("div");
  document.body.append(target);
  app = mount(App, { target });
});

afterEach(() => {
  viewMenu.close();
  arrangement.arrange(defaultArrangement());
  layout.setCenterHeight(0);
  layout.setSidebarHidden(false);
  layout.setBottomHidden(false);
  ui.resetFocus();
  flushSync();
});

afterAll(() => {
  if (app) void unmount(app);
  app = null;
});

describe("a project", () => {
  it("the default screen: the sidebar's tabs Files | Search | the plugins', the Explorer shown, the others kept behind it", async () => {
    await store.open(MOCK_FOLDER, "thisTab");
    await until(() => slot(FILES)?.querySelector('[role="tree"]') != null, "the Explorer shows");
    const strips = region("sidebar")?.querySelectorAll('[role="tablist"]') ?? [];
    expect(strips).toHaveLength(1);
    expect(strips[0]?.getAttribute("aria-label")).toBe("Sidebar views");
    const tabs = [...(strips[0]?.querySelectorAll('[role="tab"]') ?? [])];
    expect(tabs.slice(0, 2).map((tab) => tab.getAttribute("title"))).toEqual(["Files", "Search"]);
    expect(tabs[0]?.getAttribute("aria-selected")).toBe("true");
    expect(regionAt(slot(FILES))).toBe("sidebar");
    expect(slot(FILES)?.inert).not.toBe(true);
    // Search is alive behind the Explorer: its box is there, out of reach
    expect(slot(SEARCH)?.inert).toBe(true);
    expect(slot(SEARCH)?.querySelector('input[type="search"]')).not.toBeNull();
    // no view of the sidebar carries an options button: the strip has the one, for the tab it shows, in the Tab
    // order after the tabs
    expect(region("sidebar")?.querySelector("[data-view-host] [data-view-menu]")).toBeNull();
    const [more, ...others] = stripOptions("sidebar");
    expect(others).toEqual([]);
    expect(more?.getAttribute("data-view-menu")).toBe(FILES);
    expect(more?.getAttribute("aria-label")).toBe("Files options");
    expect(more?.tabIndex).toBe(0);
    expect(more?.classList.contains("options")).toBe(true);
    // one line says the moves for the whole window
    expect(document.querySelectorAll("[data-view-status]")).toHaveLength(1);
    expect(sidebarToggle()).not.toBeNull();
    expect(region("sidebar")?.getAttribute("aria-label")).toBe("Sidebar");
  });

  it("the outline at the right, alone in its group: its own header, no tab strip", async () => {
    showOutline();
    await until(() => slot(OUTLINE)?.querySelector("[data-view-header]") != null, "the outline");
    expect(regionAt(slot(OUTLINE))).toBe("right");
    expect(slot(OUTLINE)?.inert).not.toBe(true);
    // alone in its group (the default arrangement), with a header of its own: no tab names it
    expect(arrangement.groupWith(OUTLINE)?.views).toEqual([OUTLINE]);
    expect(document.querySelector(`[role="tab"][data-view="${OUTLINE}"]`)).toBeNull();
    expect(region("right")?.getAttribute("aria-label")).toBe("Right panel");
  });

  it("views alone in their groups at the right carry their options in their own headers; as tabs the strip has them", async () => {
    showOutline();
    const PROBE = "probe.panel";
    const remove = contributions.register({
      kind: "panel",
      id: PROBE,
      owner: "live-feed",
      side: "right",
      label: "live.label",
      requires: NO_REQUIREMENT,
      view: () => import("../testing/ViewProbe.svelte"),
    });
    layout.setOpen(PROBE, true);
    const right = (...groups: string[][]): void => {
      arrangement.arrange({
        ...defaultArrangement(),
        right: groups.map((views) => ({ views, share: 1 / groups.length })),
      });
      flushSync();
    };
    right([OUTLINE], [PROBE]);
    await until(() => headerOptions(PROBE) !== null, "the panel's options");
    expect(headerOptions(OUTLINE)?.getAttribute("aria-label")).toBe("Outline options");
    expect(headerOptions(PROBE)?.getAttribute("aria-label")).toBe("Live changes options");
    expect(stripOptions("right")).toEqual([]);
    right([OUTLINE, PROBE]);
    expect([headerOptions(OUTLINE), headerOptions(PROBE)]).toEqual([null, null]);
    expect(stripOptions("right").map((button) => button.getAttribute("data-view-menu"))).toEqual([
      OUTLINE,
    ]);
    layout.setOpen(PROBE, false);
    remove();
    flushSync();
  });

  it("the outline's options: Move to left makes it the tab shown in the Files group; Reset layout brings it back", async () => {
    showOutline();
    await until(() => headerOptions(OUTLINE) !== null, "the outline's options");
    const header = slot(OUTLINE)?.querySelector("[data-view-header]");
    headerOptions(OUTLINE)?.click();
    flushSync();
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "Outline options",
    );
    await pick("Move to sidebar");
    expect(arrangement.saved.left).toHaveLength(1);
    expect(arrangement.saved.left[0]?.views.slice(0, 2)).toEqual([FILES, SEARCH]);
    expect(arrangement.saved.left[0]?.views.at(-1)).toBe(OUTLINE);
    expect(arrangement.regionOf(OUTLINE)).toBe("left");
    expect(arrangement.isSelected(OUTLINE)).toBe(true);
    // the same outline, moved
    expect(regionAt(slot(OUTLINE))).toBe("sidebar");
    expect(slot(OUTLINE)?.querySelector("[data-view-header]")).toBe(header);
    expect(document.querySelector("[data-view-status]")?.textContent).toBe(
      "Outline moved to the sidebar",
    );
    const [more] = stripOptions("sidebar");
    expect(more?.getAttribute("data-view-menu")).toBe(OUTLINE);
    expect(document.activeElement).toBe(more);

    more?.click();
    flushSync();
    await pick("Reset layout");
    expect(arrangement.atDefault).toBe(true);
    expect(regionAt(slot(OUTLINE))).toBe("right");
    expect(slot(OUTLINE)?.querySelector("[data-view-header]")).toBe(header);
    expect(document.querySelector("[data-view-status]")?.textContent).toBe(
      "Layout of the panels reset",
    );
    expect(document.activeElement).toBe(headerOptions(OUTLINE));
  });

  it("Search moved to the right keeps what was typed: the same box, never a second one", async () => {
    const box = slot(SEARCH)?.querySelector<HTMLInputElement>('input[type="search"]');
    if (!box) throw new Error("no search box");
    box.value = "ideas";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    arrangement.arrange(moveTo(arrangement.layout, SEARCH, "right"));
    flushSync();
    expect(regionAt(slot(SEARCH))).toBe("right");
    expect(slot(SEARCH)?.contains(box)).toBe(true);
    expect(box.value).toBe("ideas");
    expect(document.querySelectorAll(`[data-view-host="${SEARCH}"]`)).toHaveLength(1);
    expect(slot(SEARCH)?.querySelectorAll('input[type="search"]')).toHaveLength(1);
    // Files alone at the left has no header of its own: its strip stays
    expect(region("sidebar")?.querySelectorAll('[role="tablist"]')).toHaveLength(1);
    arrangement.arrange(defaultArrangement());
    flushSync();
    expect(regionAt(slot(SEARCH))).toBe("sidebar");
    expect(slot(SEARCH)?.contains(box)).toBe(true);
    expect(box.value).toBe("ideas");
  });

  it("no bottom region by default: no band, no splitter, no toggle, its command not offered", () => {
    expect(region("bottom")).toBeNull();
    expect(bottomBar()).toBeNull();
    expect(bottomToggle()).toBeNull();
    expect(runnable(bottomCommand(), conditions.current)).toBe(false);
  });

  it("a view arranged at the bottom brings the region under the page: band, splitter, toggle after the sidebar's", () => {
    showOutline();
    const box = slot(SEARCH)?.querySelector<HTMLInputElement>('input[type="search"]');
    arrangement.arrange(moveTo(arrangement.layout, SEARCH, "bottom"));
    flushSync();
    expect(regionAt(slot(SEARCH))).toBe("bottom");
    // the same Search, moved
    expect(box && slot(SEARCH)?.contains(box)).toBe(true);
    expect(region("bottom")?.inert).toBe(false);
    expect(region("bottom")?.querySelector('[role="tablist"]')?.getAttribute("aria-label")).toBe(
      "Bottom panel views",
    );
    // between the side columns, after the page
    expect(
      [...document.querySelectorAll("[data-region]")].map((el) => el.getAttribute("data-region")),
    ).toEqual(["sidebar", "page", "bottom", "right"]);
    expect(bottomBar()).not.toBeNull();
    expect(bottomToggle()?.getAttribute("aria-pressed")).toBe("true");
    expect(
      [...document.querySelectorAll("footer button.toggle")].map((el) =>
        el.getAttribute("aria-label"),
      ),
    ).toEqual(["Sidebar", "Bottom panel", "Right panel"]);
    expect(sidebarToggle()?.nextElementSibling).toBe(bottomToggle());
    expect(runnable(bottomCommand(), conditions.current)).toBe(true);
  });

  it("the bottom region hides | shows on its own (its command, its toggle); a view shown in it shows it again", () => {
    arrangement.arrange(moveTo(arrangement.layout, SEARCH, "bottom"));
    flushSync();
    void bottomCommand().run();
    flushSync();
    expect(layout.bottomHidden).toBe(true);
    expect([layout.sidebarHidden, layout.rightHidden]).toEqual([false, false]);
    // still there, out of reach, no room, no splitter
    expect(region("bottom")?.inert).toBe(true);
    expect(slot(SEARCH)?.querySelector('input[type="search"]')).not.toBeNull();
    expect(bottomBar()).toBeNull();
    expect(bottomToggle()?.getAttribute("aria-pressed")).toBe("false");
    expect(arrangement.onScreen(SEARCH)).toBe(false);
    bottomToggle()?.click();
    flushSync();
    expect(layout.bottomHidden).toBe(false);
    expect(region("bottom")?.inert).toBe(false);
    void bottomCommand().run();
    showView(SEARCH);
    flushSync();
    expect(layout.bottomHidden).toBe(false);
    expect(arrangement.onScreen(SEARCH)).toBe(true);
    // the last view leaves: the region goes, its toggle with it
    arrangement.arrange(defaultArrangement());
    flushSync();
    expect(region("bottom")).toBeNull();
    expect(bottomToggle()).toBeNull();
  });

  it("a window too short for the bottom region: not drawn, its toggle unpressed, its views not on screen", () => {
    arrangement.arrange(moveTo(arrangement.layout, SEARCH, "bottom"));
    flushSync();
    const center = region("bottom")?.parentElement;
    if (!center) throw new Error("no center column");
    resized(center, PAGE_MIN_H + BOTTOM.min);
    expect(region("bottom")?.inert).toBe(false);
    expect(bottomToggle()?.getAttribute("aria-pressed")).toBe("true");
    expect(arrangement.onScreen(SEARCH)).toBe(true);
    resized(center, PAGE_MIN_H + BOTTOM.min - 1);
    expect(region("bottom")?.inert).toBe(true);
    expect(bottomBar()).toBeNull();
    // still there (its toggle too), off: nothing the person chose
    expect(bottomToggle()?.getAttribute("aria-pressed")).toBe("false");
    expect(arrangement.onScreen(SEARCH)).toBe(false);
    expect(layout.bottomHidden).toBe(false);
    expect(slot(SEARCH)?.querySelector('input[type="search"]')).not.toBeNull();
    resized(center, 900);
    expect(region("bottom")?.inert).toBe(false);
    expect(bottomBar()).not.toBeNull();
    expect(bottomToggle()?.getAttribute("aria-pressed")).toBe("true");
    expect(arrangement.onScreen(SEARCH)).toBe(true);
  });

  it("a window too short for the bottom region: its toggle cannot act and says why, its command is not offered", () => {
    const why = "Make the window taller to show the bottom panel.";
    arrangement.arrange(moveTo(arrangement.layout, SEARCH, "bottom"));
    flushSync();
    const center = region("bottom")?.parentElement;
    if (!center) throw new Error("no center column");
    const describedBy = (): string =>
      document.getElementById(bottomToggle()?.getAttribute("aria-describedby") ?? "")
        ?.textContent ?? "";
    resized(center, 900);
    expect(bottomToggle()?.hasAttribute("aria-disabled")).toBe(false);
    expect(bottomToggle()?.hasAttribute("aria-describedby")).toBe(false);
    expect(bottomToggle()?.getAttribute("title")).toMatch(/^Hide bottom panel/);
    expect(runnable(bottomCommand(), conditions.current)).toBe(true);

    resized(center, PAGE_MIN_H + BOTTOM.min - 1);
    expect(bottomToggle()?.getAttribute("aria-disabled")).toBe("true");
    expect(bottomToggle()?.getAttribute("aria-label")).toBe("Bottom panel");
    expect(bottomToggle()?.getAttribute("title")).toBe(why);
    expect(describedBy()).toBe(why);
    // still a button the keyboard reaches (it says why); a press changes nothing the person chose
    expect((bottomToggle() as HTMLButtonElement | null)?.disabled).toBe(false);
    bottomToggle()?.click();
    flushSync();
    expect(layout.bottomHidden).toBe(false);
    expect(runnable(bottomCommand(), conditions.current)).toBe(false);
    // hidden by the person as well: showing it would still show nothing
    layout.setBottomHidden(true);
    flushSync();
    expect(bottomToggle()?.getAttribute("aria-disabled")).toBe("true");
    expect(bottomToggle()?.getAttribute("title")).toBe(why);
    expect(runnable(bottomCommand(), conditions.current)).toBe(false);
    layout.setBottomHidden(false);

    resized(center, 900);
    expect(bottomToggle()?.hasAttribute("aria-disabled")).toBe(false);
    expect(describedBy()).toBe("");
    expect(runnable(bottomCommand(), conditions.current)).toBe(true);
    // the other toggles never say it
    expect(sidebarToggle()?.hasAttribute("aria-disabled")).toBe(false);
  });

  it("a view moved to a bottom region the window is too short to show: said with the way to see it", async () => {
    showOutline();
    await until(() => headerOptions(OUTLINE) !== null, "the outline's options");
    const center = document.querySelector('[data-frame="center"]');
    if (!center) throw new Error("no center column");
    resized(center, PAGE_MIN_H + BOTTOM.min - 1);
    headerOptions(OUTLINE)?.click();
    flushSync();
    await pick("Move to bottom panel");
    expect(arrangement.regionOf(OUTLINE)).toBe("bottom");
    expect(region("bottom")?.inert).toBe(true);
    expect(document.querySelector("[data-view-status]")?.textContent).toBe(
      "Outline moved to the bottom panel. Make the window taller to show it.",
    );
  });

  it("focus mode folds the bottom region with the others: only the page", () => {
    arrangement.arrange(moveTo(arrangement.layout, SEARCH, "bottom"));
    flushSync();
    ui.setFocus("on");
    flushSync();
    expect(region("bottom")?.inert).toBe(true);
    expect(bottomBar()).toBeNull();
    expect(arrangement.onScreen(SEARCH)).toBe(false);
    ui.resetFocus();
    flushSync();
    expect(region("bottom")?.inert).toBe(false);
  });
});

describe("a single file", () => {
  it("has no left region by default; the outline arranged at the left brings it, with its toggle", async () => {
    await store.open("C:/p/one-file.md", "thisTab");
    await until(
      () => store.view.state === "showing" && store.view.workspace.kind === "singleFile",
      "the file shows",
    );
    await until(() => conditions.current.workspace === "singleFile", "the context follows");
    await until(() => documents.state.status !== "loading", "the document settled");
    showOutline();
    expect(region("sidebar")).toBeNull();
    expect(sidebarToggle()).toBeNull();
    expect(runnable(toggleCommand(), conditions.current)).toBe(false);
    expect(regionAt(slot(OUTLINE))).toBe("right");
    const outline = slot(OUTLINE)?.querySelector("[data-view-header]");
    // the outline alone in the window: the screen as it always was, no options button; a right click on its
    // header still opens its options
    expect(arrangement.presentCount).toBe(1);
    expect(document.querySelector("[data-view-menu]")).toBeNull();
    outline?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();
    expect(menuItems().map((item) => item.textContent?.trim())).toEqual([
      "Move to sidebar",
      "Move to bottom panel",
      "Close",
      "Layouts…",
      "Reset layout",
    ]);
    expect(menuItems().at(-1)?.getAttribute("aria-disabled")).toBe("true");
    viewMenu.close();
    flushSync();

    outlineAtLeft();
    expect(regionAt(slot(OUTLINE))).toBe("sidebar");
    // the same outline, moved: not a second one
    expect(slot(OUTLINE)?.querySelector("[data-view-header]")).toBe(outline);
    expect(region("sidebar")?.querySelectorAll('[role="tablist"]')).toHaveLength(0);
    expect(region("sidebar")?.querySelector('[role="tree"]')).toBeNull();
    expect(sidebarToggle()).not.toBeNull();
    expect(runnable(toggleCommand(), conditions.current)).toBe(true);
    void toggleCommand().run();
    expect(layout.sidebarHidden).toBe(true);
  });
});

describe("the Notes", () => {
  it("keep their own list at the top of the left region; a view arranged at the left shows under it", async () => {
    await store.openNotes("thisTab");
    await until(() => region("sidebar")?.querySelector("aside.notes") != null, "the notes list");
    await until(() => documents.state.status !== "loading", "the document settled");
    const left = (): Element | null =>
      region("sidebar")?.querySelector("[data-view-region]") ?? null;
    expect(left()).toBeNull();
    expect(slot(FILES)).toBeNull();
    expect(sidebarToggle()).not.toBeNull();

    showOutline();
    outlineAtLeft();
    expect(left()?.getAttribute("data-view-region")).toBe("left");
    expect(regionAt(slot(OUTLINE))).toBe("sidebar");
    const list = region("sidebar")?.querySelector("aside.notes");
    // the list first, the region under it
    expect(
      list &&
        left() &&
        list.compareDocumentPosition(left() as Node) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    arrangement.arrange(defaultArrangement());
    flushSync();
    expect(left()).toBeNull();
    expect(region("sidebar")?.querySelector("aside.notes")).not.toBeNull();
  });
});
