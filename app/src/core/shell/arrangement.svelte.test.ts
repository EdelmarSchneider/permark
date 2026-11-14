// @vitest-environment jsdom
// The views' defaults (design-live-follow §14.1.3): decided once per project load, a yes at once, a no only after git
// had its time; the project's own choice always wins. The arrangement follows the settings (the three regions, else
// the right column of 0.11) and is written back; which views are present, drawn and on screen in each region.
import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Api, AppSettings, DocumentView } from "../../api";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { createMockApi } = await import("../../api/mock");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { documents } = await import("../doc/document.svelte");
const { reading } = await import("../doc/reading.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { layout } = await import("./layout.svelte");
const { ui } = await import("./ui.svelte");
const { arrangement, DEFAULT_WAIT_MS } = await import("./arrangement.svelte");
const { defaultArrangement, DIVIDER_PX, FILES, LIVE_VIEW, MIN_GROUP_PX, OUTLINE, REGIONS, SEARCH } =
  await import("./arrangement");

const LEFT_VIEW = "unread.panel";
const view = () => import("../testing/StripFirst.svelte");
/** A profile of its own per test: what one test saves, the next never reads. */
let api: Api;
let calls = false;
const removals: (() => void)[] = [];
let stop: () => void = () => {};

beforeEach(async () => {
  vi.useFakeTimers();
  calls = false;
  api = createMockApi();
  removals.push(
    contributions.register({
      kind: "panel",
      id: LIVE_VIEW,
      owner: "live-feed" as PluginId,
      side: "right",
      label: "live.label",
      requires: NO_REQUIREMENT,
      view,
      offered: () => true,
      defaultOpen: () => calls,
    }),
  );
  stop = arrangement.start(api);
  conditions.set({ ...neutralContext(), workspace: "project" });
  await api.projectSessionSet("layout.views", null);
  await layout.load(api, true);
  flushSync();
});

afterEach(async () => {
  stop();
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  documents.state = { status: "empty" };
  reading.setOutline(true);
  ui.resetFocus();
  for (const region of REGIONS) arrangement.setSize(region, 0);
  await layout.load(api, false);
  vi.useRealTimers();
});

const live = () => {
  const panel = arrangement.viewOf(LIVE_VIEW)?.panel ?? null;
  if (panel === null) throw new Error("no panel");
  return panel;
};

/** A panel of the left side, as a plugin adds one. */
function addLeftPanel(offered = true): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id: LEFT_VIEW,
      owner: "unread" as PluginId,
      side: "left",
      label: "live.label",
      requires: NO_REQUIREMENT,
      view,
      offered: () => offered,
    }),
  );
  flushSync();
}

/** A document with a heading is shown (the outline has something to list). */
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

/** The app settings as the backend holds them now. */
async function settings(): Promise<AppSettings> {
  const saved = await api.settingsGet();
  if (!saved.ok) throw new Error("no settings");
  return saved.value;
}

const viewsOf = (groups: readonly { readonly views: readonly string[] }[]): string[][] =>
  groups.map((g) => [...g.views]);

describe("a view's default", () => {
  it("a yes while git may still answer is taken at once and kept for the load", () => {
    expect(arrangement.isOpen(live())).toBe(false);
    calls = true;
    expect(arrangement.isOpen(live())).toBe(true);
    calls = false;
    expect(arrangement.isOpen(live())).toBe(true);
  });

  it("a no is kept once git had its time: a late yes opens nothing this load, the next load decides", async () => {
    vi.advanceTimersByTime(DEFAULT_WAIT_MS + 10);
    flushSync();
    expect(arrangement.isOpen(live())).toBe(false);
    calls = true;
    expect(arrangement.isOpen(live())).toBe(false);
    await layout.load(api, true);
    flushSync();
    expect(arrangement.isOpen(live())).toBe(true);
  });

  it("the project's own choice wins over the default, both ways", () => {
    calls = true;
    layout.setOpen(LIVE_VIEW, false);
    expect(arrangement.isOpen(live())).toBe(false);
    calls = false;
    layout.setOpen(LIVE_VIEW, true);
    expect(arrangement.isOpen(live())).toBe(true);
  });

  it("outside a project: never open by default", () => {
    conditions.set({ ...neutralContext(), workspace: "notes" });
    calls = true;
    expect(arrangement.isOpen(live())).toBe(false);
  });
});

describe("the arrangement", () => {
  it("is drawn at once and written a moment after the last change, as the three regions", async () => {
    const before = await settings();
    expect([before.panels, before.rightViews]).toEqual([null, null]);
    const tabs = {
      v: 2 as const,
      left: defaultArrangement().left,
      right: [{ views: [OUTLINE, LIVE_VIEW], share: 1 }],
      bottom: [],
    };
    arrangement.arrange(tabs);
    expect(arrangement.saved).toBe(tabs);
    expect((await settings()).panels).toBeNull();
    await vi.advanceTimersByTimeAsync(450);
    const after = await settings();
    expect(after.panels).toEqual({
      v: 2,
      left: [{ views: [FILES, SEARCH], share: 1 }],
      right: [{ views: [OUTLINE, LIVE_VIEW], share: 1 }],
      bottom: [],
    });
    // the key of 0.11 is never written
    expect(after.rightViews).toBeNull();
  });

  it("nothing is written until the person changes something", async () => {
    const write = vi.spyOn(api, "settingsSet");
    await vi.advanceTimersByTimeAsync(2000);
    stop();
    stop = arrangement.start(api);
    await vi.advanceTimersByTimeAsync(2000);
    stop();
    stop = () => {};
    expect(write).not.toHaveBeenCalled();
  });

  it("reads the three regions when they are saved, else the right column of 0.11 beside the default left", async () => {
    const old = {
      v: 1,
      groups: [
        { views: [LIVE_VIEW], share: 0.25 },
        { views: [OUTLINE], share: 0.75 },
      ],
    };
    // a profile of 0.11 with its own right column, read when the window starts
    stop();
    api = createMockApi();
    await api.settingsSet({ key: "rightViews", value: old });
    expect(arrangement.saved).toEqual(defaultArrangement());
    const write = vi.spyOn(api, "settingsSet");
    stop = arrangement.start(api);
    await vi.advanceTimersByTimeAsync(1000);
    expect(arrangement.saved).toEqual({
      v: 2,
      left: [{ views: [FILES, SEARCH], share: 1 }],
      right: old.groups,
      bottom: [],
    });
    // reading writes no key; the old key stays as it was
    expect(write).not.toHaveBeenCalled();
    expect((await settings()).rightViews).toEqual(old);
    write.mockRestore();

    const panels = {
      v: 2,
      left: [{ views: [SEARCH, FILES], share: 1 }],
      right: [{ views: [OUTLINE], share: 1 }],
      bottom: [{ views: [LIVE_VIEW], share: 1 }],
    };
    await api.settingsSet({ key: "panels", value: panels });
    expect(arrangement.saved).toEqual(panels);
    // a value this build cannot read (a newer format): the column of 0.11 again
    await api.settingsSet({ key: "panels", value: { ...panels, v: 3 } });
    expect(viewsOf(arrangement.saved.left)).toEqual([[FILES, SEARCH]]);
    expect(arrangement.saved.right).toEqual(old.groups);
    expect(arrangement.saved.bottom).toEqual([]);
  });

  it("a change made from the old column keeps it: the three regions are written, the old key is not touched", async () => {
    const old = {
      v: 1,
      groups: [
        { views: [LIVE_VIEW], share: 0.25 },
        { views: [OUTLINE], share: 0.75 },
      ],
    };
    await api.settingsSet({ key: "rightViews", value: old });
    const write = vi.spyOn(api, "settingsSet");
    arrangement.arrange(arrangement.layout);
    await vi.advanceTimersByTimeAsync(450);
    expect(write.mock.calls.map(([change]) => change.key)).toEqual(["panels"]);
    const after = await settings();
    expect(after.rightViews).toEqual(old);
    expect(after.panels).toEqual({
      v: 2,
      left: [{ views: [FILES, SEARCH], share: 1 }],
      right: old.groups,
      bottom: [],
    });
  });
});

describe("the views of the window", () => {
  it("every view has a default region; only the outline and the right panels can be closed", () => {
    addLeftPanel();
    expect(arrangement.views.map((v) => [v.id, v.region, v.closable])).toEqual([
      [FILES, "left", false],
      [SEARCH, "left", false],
      [LEFT_VIEW, "left", false],
      [OUTLINE, "right", true],
      [LIVE_VIEW, "right", true],
    ]);
    expect(arrangement.viewOf(LEFT_VIEW)?.panel?.id).toBe(LEFT_VIEW);
    expect(arrangement.viewOf(FILES)?.panel).toBeNull();
    expect(arrangement.viewOf("gone")).toBeUndefined();
    // a view no region lists joins the last group of its default region (what is saved stays as saved)
    expect(viewsOf(arrangement.layout.left)).toEqual([[FILES, SEARCH, LEFT_VIEW]]);
    expect(viewsOf(arrangement.layout.right)).toEqual([[OUTLINE], [LIVE_VIEW]]);
    expect(viewsOf(arrangement.saved.left)).toEqual([[FILES, SEARCH]]);
  });

  it("Files, Search and the left panels are present in a project only; the outline and open panels anywhere", () => {
    addLeftPanel();
    showOutline();
    layout.setOpen(LIVE_VIEW, true);
    const present = () =>
      [FILES, SEARCH, LEFT_VIEW, OUTLINE, LIVE_VIEW].filter((id) => arrangement.isPresent(id));
    expect(present()).toEqual([FILES, SEARCH, LEFT_VIEW, OUTLINE, LIVE_VIEW]);
    expect(arrangement.presentCount).toBe(5);
    expect(viewsOf(arrangement.drawn("left"))).toEqual([[FILES, SEARCH, LEFT_VIEW]]);
    expect(viewsOf(arrangement.drawn("right"))).toEqual([[OUTLINE], [LIVE_VIEW]]);
    expect(arrangement.drawn("bottom")).toEqual([]);

    for (const workspace of ["singleFile", "notes", "welcome"] as const) {
      conditions.set({ ...neutralContext(), workspace });
      expect(present(), workspace).toEqual([OUTLINE, LIVE_VIEW]);
      expect(arrangement.presentCount, workspace).toBe(2);
      expect(arrangement.drawn("left"), workspace).toEqual([]);
      expect(viewsOf(arrangement.drawn("right")), workspace).toEqual([[OUTLINE], [LIVE_VIEW]]);
    }

    // the outline: the document has one, and it is on
    reading.setOutline(false);
    expect(arrangement.isPresent(OUTLINE)).toBe(false);
    reading.setOutline(true);
    documents.state = { status: "empty" };
    expect(arrangement.isPresent(OUTLINE)).toBe(false);
    // a right panel: open; a view nobody offers is never present
    layout.setOpen(LIVE_VIEW, false);
    expect(arrangement.isPresent(LIVE_VIEW)).toBe(false);
    expect(arrangement.isPresent("gone")).toBe(false);
    expect(arrangement.presentCount).toBe(0);
  });

  it("a left panel its plugin does not offer is not a view of the window", () => {
    addLeftPanel(false);
    expect(arrangement.viewOf(LEFT_VIEW)).toBeUndefined();
    expect(arrangement.isPresent(LEFT_VIEW)).toBe(false);
    expect(viewsOf(arrangement.layout.left)).toEqual([[FILES, SEARCH]]);
  });

  it("the selected view of a group, over every region", () => {
    showOutline();
    layout.setOpen(LIVE_VIEW, true);
    expect(arrangement.groupWith(SEARCH)?.views).toEqual([FILES, SEARCH]);
    expect(arrangement.groupWith(LIVE_VIEW)?.views).toEqual([LIVE_VIEW]);
    expect(arrangement.groupWith("gone")).toBeUndefined();
    expect([FILES, SEARCH, OUTLINE, LIVE_VIEW].map((id) => arrangement.isSelected(id))).toEqual([
      true,
      false,
      true,
      true,
    ]);
    layout.setSelected([SEARCH]);
    expect([FILES, SEARCH].map((id) => arrangement.isSelected(id))).toEqual([false, true]);
  });

  it("on screen: selected in its group, its own region not hidden, not in focus mode", () => {
    showOutline();
    layout.setOpen(LIVE_VIEW, true);
    arrangement.arrange({
      v: 2,
      left: [{ views: [FILES, SEARCH], share: 1 }],
      right: [{ views: [OUTLINE], share: 1 }],
      bottom: [{ views: [LIVE_VIEW], share: 1 }],
    });
    const shown = () => [FILES, OUTLINE, LIVE_VIEW].map((id) => arrangement.onScreen(id));
    expect(shown()).toEqual([true, true, true]);
    // a tab behind another is not on screen
    expect(arrangement.onScreen(SEARCH)).toBe(false);

    layout.setSidebarHidden(true);
    expect(shown()).toEqual([false, true, true]);
    layout.setSidebarHidden(false);
    layout.setRightHidden(true);
    expect(shown()).toEqual([true, false, true]);
    layout.setRightHidden(false);
    layout.setBottomHidden(true);
    expect(shown()).toEqual([true, true, false]);
    layout.setBottomHidden(false);

    ui.focus = "on";
    expect(shown()).toEqual([false, false, false]);
    ui.focus = "off";
    expect(shown()).toEqual([true, true, true]);
  });

  it("a region too short for its groups draws them as one tab group, each region by its own size", () => {
    showOutline();
    layout.setOpen(LIVE_VIEW, true);
    arrangement.arrange({
      v: 2,
      left: [
        { views: [FILES], share: 0.5 },
        { views: [SEARCH], share: 0.5 },
      ],
      right: [
        { views: [OUTLINE], share: 0.5 },
        { views: [LIVE_VIEW], share: 0.5 },
      ],
      bottom: [],
    });
    const need = 2 * MIN_GROUP_PX + DIVIDER_PX;
    // nothing measured yet: as arranged
    expect([arrangement.short("left"), arrangement.short("right")]).toEqual([false, false]);
    expect(viewsOf(arrangement.drawn("right"))).toEqual([[OUTLINE], [LIVE_VIEW]]);

    arrangement.setSize("right", need - 1);
    arrangement.setSize("left", need);
    expect([arrangement.short("left"), arrangement.short("right")]).toEqual([false, true]);
    expect(arrangement.drawn("right")).toEqual([{ at: 0, views: [OUTLINE, LIVE_VIEW], share: 1 }]);
    expect(viewsOf(arrangement.stacked("right"))).toEqual([[OUTLINE], [LIVE_VIEW]]);
    expect(viewsOf(arrangement.drawn("left"))).toEqual([[FILES], [SEARCH]]);
    // the arrangement itself stays as saved
    expect(viewsOf(arrangement.saved.right)).toEqual([[OUTLINE], [LIVE_VIEW]]);

    arrangement.setSize("right", need);
    arrangement.setSize("left", 100);
    expect([arrangement.short("left"), arrangement.short("right")]).toEqual([true, false]);
    expect(arrangement.drawn("left")).toEqual([{ at: 0, views: [FILES, SEARCH], share: 1 }]);
    expect(viewsOf(arrangement.drawn("right"))).toEqual([[OUTLINE], [LIVE_VIEW]]);

    // one drawn group never folds
    layout.setOpen(LIVE_VIEW, false);
    arrangement.setSize("right", 50);
    expect(arrangement.short("right")).toBe(false);
    expect(viewsOf(arrangement.drawn("right"))).toEqual([[OUTLINE]]);
  });
});
