// @vitest-environment jsdom
// The whole window on the mock: the frame draws a region where a view is present, whatever the kind of the workspace.
// A project's default screen (sidebar tabs, one view shown), a view moved to the other region with what it holds, a
// single file, the Notes with their own list over the views arranged at the left.
import { flushSync, mount, unmount } from "svelte";
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
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};

const { api } = await import("../../api");
const { windowStore: store } = await import("../window.svelte");
const { default: App } = await import("../../App.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { runnable } = await import("../commands/commands");
const { conditions } = await import("../conditions/context.svelte");
const { documents } = await import("../doc/document.svelte");
const { layout } = await import("./layout.svelte");
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
  arrangement.arrange(defaultArrangement());
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
    // no view of the sidebar carries an options button; one line says the moves for the whole window
    expect(region("sidebar")?.querySelector("[data-view-menu]")).toBeNull();
    expect(document.querySelectorAll("[data-view-status]")).toHaveLength(1);
    expect(sidebarToggle()).not.toBeNull();
  });

  it("the outline at the right, alone in its group: its own header, no tab strip", async () => {
    showOutline();
    await until(() => slot(OUTLINE)?.querySelector("[data-view-header]") != null, "the outline");
    expect(regionAt(slot(OUTLINE))).toBe("right");
    expect(slot(OUTLINE)?.inert).not.toBe(true);
    // alone in its group (the default arrangement), with a header of its own: no tab names it
    expect(arrangement.groupWith(OUTLINE)?.views).toEqual([OUTLINE]);
    expect(document.querySelector(`[role="tab"][data-view="${OUTLINE}"]`)).toBeNull();
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
