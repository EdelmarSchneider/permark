import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import {
  BOTTOM,
  clampWidth,
  fit,
  fitBottom,
  keyWidth,
  outlineShown,
  PAGE_MIN,
  PAGE_MIN_H,
  panelShown,
  RIGHT,
  regionAvailable,
  regionShown,
  SIDEBAR,
  toggleOutline,
  togglePanel,
  toggleRegion,
  type RegionState,
  type SidePanel,
} from "./layout";
import { LayoutStore } from "./layout.svelte";

describe("panel widths", () => {
  it("clamps to the range, rounds, and takes the default for what is not a number", () => {
    expect(clampWidth(SIDEBAR, 10)).toBe(SIDEBAR.min);
    expect(clampWidth(SIDEBAR, 9999)).toBe(SIDEBAR.max);
    expect(clampWidth(SIDEBAR, 300.6)).toBe(301);
    expect(clampWidth(SIDEBAR, Number.NaN)).toBe(SIDEBAR.def);
    expect(clampWidth(RIGHT, Number.POSITIVE_INFINITY)).toBe(RIGHT.def);
  });

  it("fits the window: the page keeps its minimum, the right panel gives way first", () => {
    expect(fit(1400, 300, 300)).toEqual({ sidebar: 300, right: 300 });
    const tight = fit(PAGE_MIN + 300 + 240, 300, 300);
    expect(tight).toEqual({ sidebar: 300, right: 240 });
    // still too small: the sidebar gives way too, down to its minimum
    const tiny = fit(PAGE_MIN + SIDEBAR.min + RIGHT.min - 50, 300, 300);
    expect(tiny).toEqual({ sidebar: SIDEBAR.min, right: RIGHT.min });
    // a window smaller than the minimums keeps them (the page shrinks)
    expect(fit(100, 300, 300)).toEqual({ sidebar: SIDEBAR.min, right: RIGHT.min });
    // no panel, no width; an unknown window size leaves the widths as asked
    expect(fit(900, null, 300)).toEqual({ sidebar: null, right: 300 });
    expect(fit(0, 300, 300)).toEqual({ sidebar: 300, right: 300 });
    expect(fit(Number.NaN, 300, null)).toEqual({ sidebar: 300, right: null });
  });

  it("narrow windows: the panels yield before the page drops under its minimum", () => {
    // a common laptop window with both panels at their defaults: the right one gives way, the page keeps 560
    const total = SIDEBAR.def + RIGHT.min + PAGE_MIN;
    const laptop = fit(total, SIDEBAR.def, RIGHT.def);
    expect(laptop).toEqual({ sidebar: SIDEBAR.def, right: RIGHT.min });
    expect(total - (laptop.sidebar ?? 0) - (laptop.right ?? 0)).toBe(PAGE_MIN);
    // one pixel more or less around the point where both panels sit at their minimums
    const edge = PAGE_MIN + SIDEBAR.min + RIGHT.min;
    expect(fit(edge, 400, 400)).toEqual({ sidebar: SIDEBAR.min, right: RIGHT.min });
    expect(fit(edge + 1, 400, 400)).toEqual({ sidebar: SIDEBAR.min + 1, right: RIGHT.min });
    expect(fit(edge - 1, 400, 400)).toEqual({ sidebar: SIDEBAR.min, right: RIGHT.min });
    // only one panel: it alone gives way
    expect(fit(PAGE_MIN + 250, null, 400)).toEqual({ sidebar: null, right: 250 });
    expect(fit(PAGE_MIN + 250, 400, null)).toEqual({ sidebar: 250, right: null });
  });

  it("the bottom panel's height: the page keeps its minimum height, the panel gives way down to its own", () => {
    expect([BOTTOM.min, BOTTOM.def, PAGE_MIN_H]).toEqual([120, 240, 220]);
    // room for all: as asked (inside its range)
    expect(fitBottom(900, 240)).toBe(240);
    expect(fitBottom(900, 300.4)).toBe(300);
    expect(fitBottom(900, 10)).toBe(BOTTOM.min);
    expect(fitBottom(PAGE_MIN_H + 240, 240)).toBe(240);
    // a short window: the panel is squeezed, the page keeps its 220
    expect(fitBottom(PAGE_MIN_H + 239, 240)).toBe(239);
    expect(fitBottom(400, 240)).toBe(180);
    expect(fitBottom(PAGE_MIN_H + BOTTOM.min, 600)).toBe(BOTTOM.min);
    // a height past what any window holds is cut to the room left
    expect(fitBottom(700, 99999)).toBe(700 - PAGE_MIN_H);
    // not even its minimum fits: not drawn
    expect(fitBottom(PAGE_MIN_H + BOTTOM.min - 1, 240)).toBeNull();
    expect(fitBottom(100, 240)).toBeNull();
    // no bottom panel, no height; an unknown window size leaves the height as asked
    expect(fitBottom(900, null)).toBeNull();
    expect(fitBottom(0, 240)).toBe(240);
    expect(fitBottom(Number.NaN, 240)).toBe(240);
    expect(fitBottom(900, Number.NaN)).toBe(BOTTOM.def);
  });

  it("keys: arrows by 16 | 64 with Shift, Home | End = limits, Enter = default, others pass", () => {
    expect(keyWidth("ArrowRight", false, 260, SIDEBAR, "right")).toBe(276);
    expect(keyWidth("ArrowLeft", false, 260, SIDEBAR, "right")).toBe(244);
    expect(keyWidth("ArrowRight", true, 260, SIDEBAR, "right")).toBe(324);
    // the right panel grows to the left
    expect(keyWidth("ArrowLeft", false, 280, RIGHT, "left")).toBe(296);
    expect(keyWidth("ArrowRight", false, 280, RIGHT, "left")).toBe(264);
    expect(keyWidth("ArrowLeft", true, 280, SIDEBAR, "right")).toBe(216);
    expect(keyWidth("ArrowLeft", true, 210, SIDEBAR, "right")).toBe(SIDEBAR.min);
    expect(keyWidth("Home", false, 300, SIDEBAR, "right")).toBe(SIDEBAR.min);
    expect(keyWidth("End", false, 300, SIDEBAR, "right")).toBe(SIDEBAR.max);
    expect(keyWidth("Enter", false, 300, SIDEBAR, "right")).toBe(SIDEBAR.def);
    expect(keyWidth("a", false, 300, SIDEBAR, "right")).toBeNull();
    expect(keyWidth("Tab", false, 300, SIDEBAR, "right")).toBeNull();
  });

  it("keys of the bottom panel's edge: Up makes it taller, Down shorter; each edge takes only its own arrows", () => {
    expect(keyWidth("ArrowUp", false, 240, BOTTOM, "up")).toBe(256);
    expect(keyWidth("ArrowDown", false, 240, BOTTOM, "up")).toBe(224);
    expect(keyWidth("ArrowUp", true, 240, BOTTOM, "up")).toBe(304);
    expect(keyWidth("ArrowDown", true, 150, BOTTOM, "up")).toBe(BOTTOM.min);
    expect(keyWidth("Home", false, 300, BOTTOM, "up")).toBe(BOTTOM.min);
    expect(keyWidth("End", false, 300, BOTTOM, "up")).toBe(BOTTOM.max);
    expect(keyWidth("Enter", false, 300, BOTTOM, "up")).toBe(BOTTOM.def);
    // across its bar: not its keys (they stay with whatever else reads them)
    expect(keyWidth("ArrowLeft", false, 240, BOTTOM, "up")).toBeNull();
    expect(keyWidth("ArrowRight", true, 240, BOTTOM, "up")).toBeNull();
    expect(keyWidth("ArrowUp", false, 260, SIDEBAR, "right")).toBeNull();
    expect(keyWidth("ArrowDown", false, 280, RIGHT, "left")).toBeNull();
  });
});

describe("the layout store", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("a project keeps its widths in the project session, written a moment after the last change", async () => {
    const api = createMockApi();
    await api.projectSessionSet("layout.sidebar", 333);
    await api.projectSessionSet("layout.right", 9999);
    const store = new LayoutStore();
    await store.load(api, true);
    expect(store.sidebar).toBe(333);
    expect(store.right).toBe(RIGHT.max);
    store.setSidebar(350);
    store.setSidebar(360);
    store.setSidebar(10);
    expect(store.sidebar).toBe(SIDEBAR.min);
    const before = await api.projectSessionGet("layout.sidebar");
    expect(before.ok && before.value).toBe(333);
    await vi.advanceTimersByTimeAsync(450);
    const saved = await api.projectSessionGet("layout.sidebar");
    expect(saved.ok && saved.value).toBe(SIDEBAR.min);
  });

  it("garbage in the session reads as the defaults; a single file keeps nothing", async () => {
    const api = createMockApi();
    await api.projectSessionSet("layout.sidebar", "wide");
    await api.projectSessionSet("layout.right", { a: 1 });
    const store = new LayoutStore();
    await store.load(api, true);
    expect([store.sidebar, store.right]).toEqual([SIDEBAR.def, RIGHT.def]);
    await store.load(api, false);
    store.setRight(400);
    await vi.advanceTimersByTimeAsync(1000);
    const saved = await api.projectSessionGet("layout.right");
    expect(saved.ok && saved.value).toEqual({ a: 1 });
    expect(store.right).toBe(400);
  });

  it("the bottom panel's height is kept like the widths: read, clamped, written; junk reads as the default", async () => {
    const api = createMockApi();
    await api.projectSessionSet("layout.bottom", 333);
    const store = new LayoutStore();
    expect(store.bottom).toBe(BOTTOM.def);
    await store.load(api, true);
    expect(store.bottom).toBe(333);
    store.setBottom(10);
    expect(store.bottom).toBe(BOTTOM.min);
    store.setBottom(400.4);
    expect(store.bottom).toBe(400);
    const before = await api.projectSessionGet("layout.bottom");
    expect(before.ok && before.value).toBe(333);
    await vi.advanceTimersByTimeAsync(450);
    const saved = await api.projectSessionGet("layout.bottom");
    expect(saved.ok && saved.value).toBe(400);
    // the other sizes are written beside it, as they were
    const sidebar = await api.projectSessionGet("layout.sidebar");
    expect(sidebar.ok && sidebar.value).toBe(SIDEBAR.def);
    for (const junk of ["tall", { a: 1 }, [300], true, null]) {
      await api.projectSessionSet("layout.bottom", junk);
      const again = new LayoutStore();
      await again.load(api, true);
      expect(again.bottom, JSON.stringify(junk)).toBe(BOTTOM.def);
    }
    await api.projectSessionSet("layout.bottom", 99999);
    await store.load(api, true);
    expect(store.bottom).toBe(BOTTOM.max);
    // a single file keeps it for the window only; the next workspace starts at the default
    await store.load(api, false);
    expect(store.bottom).toBe(BOTTOM.def);
    store.setBottom(300);
    await vi.advanceTimersByTimeAsync(1000);
    const kept = await api.projectSessionGet("layout.bottom");
    expect(kept.ok && kept.value).toBe(99999);
    expect(store.bottom).toBe(300);
  });

  it("a workspace that was left cannot overwrite the next one with its widths", async () => {
    const api = createMockApi();
    const store = new LayoutStore();
    await store.load(api, true);
    store.setSidebar(400);
    await store.load(api, true);
    await vi.advanceTimersByTimeAsync(1000);
    const saved = await api.projectSessionGet("layout.sidebar");
    expect(saved.ok && saved.value).toBeNull();
    expect(store.sidebar).toBe(SIDEBAR.def);
  });
});

describe("showing | hiding a region", () => {
  const live = (over: Partial<SidePanel> = {}): SidePanel => ({
    id: "live-feed.panel",
    open: false,
    selected: true,
    ...over,
  });
  const side = (over: Partial<RegionState> = {}): RegionState => ({
    fixed: false,
    hasOutline: true,
    outlineOn: true,
    outlineSelected: true,
    panels: [],
    hidden: false,
    ...over,
  });

  it("shows when a view is present (the outline when on, an open panel); hidden shows nothing", () => {
    expect(regionShown(side())).toBe(true);
    expect(regionShown(side({ outlineOn: false }))).toBe(false);
    expect(regionShown(side({ outlineOn: false, panels: [live({ open: true })] }))).toBe(true);
    expect(regionShown(side({ outlineOn: false, panels: [live()] }))).toBe(false);
    expect(regionShown(side({ hasOutline: false }))).toBe(false);
    expect(regionShown(side({ hidden: true }))).toBe(false);
    expect(regionShown(side({ hidden: true, panels: [live({ open: true })] }))).toBe(false);
    expect(regionAvailable(side({ hasOutline: false }))).toBe(false);
    expect(regionAvailable(side({ hasOutline: false, panels: [live()] }))).toBe(true);
    expect(regionAvailable(side({ hidden: true, outlineOn: false }))).toBe(true);
  });

  it("the toggle hides what shows, shows what is hidden, and never does nothing visible", () => {
    expect(toggleRegion(side())).toEqual({ hidden: true });
    expect(toggleRegion(side({ hidden: true }))).toEqual({ hidden: false });
    // shown again with nothing present: the outline turns on
    expect(toggleRegion(side({ outlineOn: false }))).toEqual({
      hidden: false,
      outlineOn: true,
      select: "outline",
    });
    expect(toggleRegion(side({ hidden: true, outlineOn: false }))).toEqual({
      hidden: false,
      outlineOn: true,
      select: "outline",
    });
    // an open panel: it shows again, the outline choice stays
    expect(
      toggleRegion(side({ hidden: true, outlineOn: false, panels: [live({ open: true })] })),
    ).toEqual({ hidden: false });
    // no outline, nothing open: the first offered panel opens
    expect(toggleRegion(side({ hasOutline: false, panels: [live()] }))).toEqual({
      hidden: false,
      open: { id: "live-feed.panel", open: true },
      select: "live-feed.panel",
    });
    // nothing it could show: no change
    expect(toggleRegion(side({ hasOutline: false, hidden: true }))).toEqual({});
  });

  it("a region with a view that is always open (Files, the Notes' list): its toggle only hides | shows", () => {
    const files = side({ fixed: true, hasOutline: false });
    expect([regionAvailable(files), regionShown(files)]).toEqual([true, true]);
    expect(regionShown({ ...files, hidden: true })).toBe(false);
    expect(toggleRegion(files)).toEqual({ hidden: true });
    expect(toggleRegion({ ...files, hidden: true })).toEqual({ hidden: false });
    // an outline arranged beside it that is off stays off: the region had something to show
    expect(toggleRegion(side({ fixed: true, outlineOn: false }))).toEqual({ hidden: true });
    expect(toggleRegion(side({ fixed: true, outlineOn: false, hidden: true }))).toEqual({
      hidden: false,
    });
    expect(toggleRegion(side({ fixed: true, hasOutline: false, panels: [live()] }))).toEqual({
      hidden: true,
    });
  });

  it("the outline toggle: on screen -> off; else on, selected, the region shown (an open panel stays open)", () => {
    expect(outlineShown(side())).toBe(true);
    expect(outlineShown(side({ outlineSelected: false }))).toBe(false);
    expect(outlineShown(side({ hidden: true }))).toBe(false);
    expect(toggleOutline(side())).toEqual({ outlineOn: false });
    for (const over of [
      { hidden: true, outlineOn: false },
      { hidden: true },
      { outlineOn: false },
      { outlineSelected: false, panels: [live({ open: true })] },
    ]) {
      expect(toggleOutline(side(over)), JSON.stringify(over)).toEqual({
        hidden: false,
        outlineOn: true,
        select: "outline",
      });
    }
  });

  it("a panel toggle: on screen -> closed; open elsewhere -> brought forward; closed -> opened", () => {
    const id = "live-feed.panel";
    expect(panelShown(side({ panels: [live({ open: true })] }), id)).toBe(true);
    expect(togglePanel(side({ panels: [live({ open: true })] }), id)).toEqual({
      open: { id, open: false },
    });
    // a tab behind another, or the region hidden: never closed, brought forward
    expect(togglePanel(side({ panels: [live({ open: true, selected: false })] }), id)).toEqual({
      hidden: false,
      select: id,
    });
    expect(togglePanel(side({ hidden: true, panels: [live({ open: true })] }), id)).toEqual({
      hidden: false,
      select: id,
    });
    expect(togglePanel(side({ panels: [live()] }), id)).toEqual({
      hidden: false,
      open: { id, open: true },
      select: id,
    });
    // not offered (dormant): opened all the same (the person asked)
    expect(togglePanel(side(), id)).toEqual({
      hidden: false,
      open: { id, open: true },
      select: id,
    });
  });

  it("rapid toggles alternate and end where an even | odd count says", () => {
    let now = side();
    for (let i = 0; i < 101; i++) now = { ...now, ...toggleRegion(now) };
    expect(regionShown(now)).toBe(false);
    now = { ...now, ...toggleRegion(now) };
    expect(regionShown(now)).toBe(true);
  });
});

describe("the right column's views in the project session", () => {
  it("are read with the layout (0.6.x sessions and junk: nothing chosen) and written after a change", async () => {
    vi.useFakeTimers();
    const api = createMockApi();
    await api.projectSessionSet("layout.views", null);
    const store = new LayoutStore();
    await store.load(api, true);
    expect(store.views).toEqual({ v: 1, open: {}, selected: [] });
    const loads = store.loads;
    store.setOpen("live-feed.panel", false);
    store.setSelected(["outline"]);
    await vi.advanceTimersByTimeAsync(450);
    const saved = await api.projectSessionGet("layout.views");
    expect(saved.ok && saved.value).toEqual({
      v: 1,
      open: { "live-feed.panel": false },
      selected: ["outline"],
    });
    for (const junk of ["x", 3, { v: 2, open: { a: true } }, { v: 1, open: { a: "yes" } }]) {
      await api.projectSessionSet("layout.views", junk);
      await store.load(api, true);
      expect(store.views.open, JSON.stringify(junk)).toEqual({});
    }
    expect(store.loads).toBe(loads + 4);
    vi.useRealTimers();
  });
});

describe("the hidden panels in the project session", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /** What 0.6.0 | 0.6.1 wrote for a project: widths, no hidden keys. */
  async function session061(api: ReturnType<typeof createMockApi>): Promise<void> {
    await api.projectSessionSet("layout.sidebar", 300);
    await api.projectSessionSet("layout.right", 320);
    await api.projectSessionSet("reading.outline", true);
  }

  it("a session of 0.6.1 opens with both panels shown and its widths kept", async () => {
    const api = createMockApi();
    await session061(api);
    const store = new LayoutStore();
    await store.load(api, true);
    expect([store.sidebarHidden, store.rightHidden]).toEqual([false, false]);
    expect([store.sidebar, store.right]).toEqual([300, 320]);
  });

  it("panels stay off while the project's session is read (no flash of a panel it hides)", async () => {
    const api = createMockApi();
    await api.projectSessionSet("layout.sidebar-hidden", true);
    const store = new LayoutStore();
    const loading = store.load(api, true);
    expect([store.sidebarOff, store.rightOff]).toEqual([true, true]);
    await loading;
    expect([store.sidebarOff, store.rightOff]).toEqual([true, false]);
    // a single file | the Notes: nothing to read
    const plain = store.load(api, false);
    expect([store.sidebarOff, store.rightOff]).toEqual([false, false]);
    await plain;
  });

  it("a hidden panel stays hidden in the project, written a moment after the last change", async () => {
    const api = createMockApi();
    await session061(api);
    const store = new LayoutStore();
    await store.load(api, true);
    store.setSidebarHidden(true);
    store.setRightHidden(true);
    store.setRightHidden(false);
    store.setRightHidden(true);
    const early = await api.projectSessionGet("layout.sidebar-hidden");
    expect(early.ok && early.value).toBeNull();
    await vi.advanceTimersByTimeAsync(450);
    const [side, right, width] = await Promise.all([
      api.projectSessionGet("layout.sidebar-hidden"),
      api.projectSessionGet("layout.right-hidden"),
      api.projectSessionGet("layout.sidebar"),
    ]);
    expect([side.ok && side.value, right.ok && right.value]).toEqual([true, true]);
    expect(width.ok && width.value).toBe(300);
    const again = new LayoutStore();
    await again.load(api, true);
    expect([again.sidebarHidden, again.rightHidden]).toEqual([true, true]);
  });

  it("the bottom panel hides on its own: off while the session is read, kept in the project, only true counts", async () => {
    const api = createMockApi();
    await session061(api);
    const store = new LayoutStore();
    const loading = store.load(api, true);
    expect(store.bottomOff).toBe(true);
    await loading;
    expect([store.bottomHidden, store.bottomOff]).toEqual([false, false]);
    store.setBottomHidden(true);
    expect([store.bottomOff, store.sidebarOff, store.rightOff]).toEqual([true, false, false]);
    const early = await api.projectSessionGet("layout.bottom-hidden");
    expect(early.ok && early.value).toBeNull();
    await vi.advanceTimersByTimeAsync(450);
    const [bottom, side, right] = await Promise.all([
      api.projectSessionGet("layout.bottom-hidden"),
      api.projectSessionGet("layout.sidebar-hidden"),
      api.projectSessionGet("layout.right-hidden"),
    ]);
    expect([bottom.ok && bottom.value, side.ok && side.value, right.ok && right.value]).toEqual([
      true,
      false,
      false,
    ]);
    const again = new LayoutStore();
    await again.load(api, true);
    expect([again.bottomHidden, again.sidebarHidden, again.rightHidden]).toEqual([
      true,
      false,
      false,
    ]);
    for (const junk of ["yes", 1, { hidden: true }, [true], null]) {
      await api.projectSessionSet("layout.bottom-hidden", junk);
      const other = new LayoutStore();
      await other.load(api, true);
      expect(other.bottomHidden, JSON.stringify(junk)).toBe(false);
    }
    // a single file | the Notes: for the window only, the next workspace starts shown
    await store.load(api, false);
    expect(store.bottomHidden).toBe(false);
    store.setBottomHidden(true);
    await vi.advanceTimersByTimeAsync(1000);
    const kept = await api.projectSessionGet("layout.bottom-hidden");
    expect(kept.ok && kept.value).toBeNull();
    expect(store.bottomHidden).toBe(true);
    await store.load(api, false);
    expect(store.bottomHidden).toBe(false);
  });

  it("a window too short for the bottom panel puts it off: the height that fits, from the room the frame reports", () => {
    const store = new LayoutStore();
    // nothing reported yet: as asked
    expect([store.bottomFit, store.bottomOff]).toEqual([BOTTOM.def, false]);
    store.setCenterHeight(PAGE_MIN_H + 180);
    expect([store.bottomFit, store.bottomOff]).toEqual([180, false]);
    store.setCenterHeight(PAGE_MIN_H + BOTTOM.min - 1);
    expect([store.bottomFit, store.bottomOff]).toEqual([null, true]);
    // not the person's choice: nothing to remember, shown again once the window is tall enough
    expect(store.bottomHidden).toBe(false);
    expect([store.sidebarOff, store.rightOff]).toEqual([false, false]);
    store.setCenterHeight(900);
    expect([store.bottomFit, store.bottomOff]).toEqual([BOTTOM.def, false]);
    store.setBottom(400);
    expect(store.bottomFit).toBe(400);
    store.setBottomHidden(true);
    expect([store.bottomFit, store.bottomOff]).toEqual([400, true]);
  });

  it("anything but true reads as shown; a single file | the Notes keep the choice for the window only", async () => {
    const api = createMockApi();
    for (const junk of ["yes", 1, { hidden: true }, [true], null]) {
      await api.projectSessionSet("layout.sidebar-hidden", junk);
      await api.projectSessionSet("layout.right-hidden", junk);
      const store = new LayoutStore();
      await store.load(api, true);
      expect([store.sidebarHidden, store.rightHidden], JSON.stringify(junk)).toEqual([
        false,
        false,
      ]);
    }
    const store = new LayoutStore();
    await store.load(api, false);
    store.setRightHidden(true);
    await vi.advanceTimersByTimeAsync(1000);
    const saved = await api.projectSessionGet("layout.right-hidden");
    expect(saved.ok && saved.value).toBeNull();
    expect(store.rightHidden).toBe(true);
    // the next workspace starts shown
    await store.load(api, false);
    expect(store.rightHidden).toBe(false);
  });
});
