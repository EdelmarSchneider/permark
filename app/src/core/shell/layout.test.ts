import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import {
  clampWidth,
  fit,
  keyWidth,
  outlineShown,
  PAGE_MIN,
  RIGHT,
  rightAvailable,
  rightShown,
  SIDEBAR,
  toggleOutline,
  toggleRight,
  type RightSide,
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

  it("keys: arrows by 16 | 64 with Shift, Home | End = limits, Enter = default, others pass", () => {
    expect(keyWidth("ArrowRight", false, 260, SIDEBAR, true)).toBe(276);
    expect(keyWidth("ArrowLeft", false, 260, SIDEBAR, true)).toBe(244);
    expect(keyWidth("ArrowRight", true, 260, SIDEBAR, true)).toBe(324);
    // the right panel grows to the left
    expect(keyWidth("ArrowLeft", false, 280, RIGHT, false)).toBe(296);
    expect(keyWidth("ArrowRight", false, 280, RIGHT, false)).toBe(264);
    expect(keyWidth("ArrowLeft", true, 280, SIDEBAR, true)).toBe(216);
    expect(keyWidth("ArrowLeft", true, 210, SIDEBAR, true)).toBe(SIDEBAR.min);
    expect(keyWidth("Home", false, 300, SIDEBAR, true)).toBe(SIDEBAR.min);
    expect(keyWidth("End", false, 300, SIDEBAR, true)).toBe(SIDEBAR.max);
    expect(keyWidth("Enter", false, 300, SIDEBAR, true)).toBe(SIDEBAR.def);
    expect(keyWidth("a", false, 300, SIDEBAR, true)).toBeNull();
    expect(keyWidth("Tab", false, 300, SIDEBAR, true)).toBeNull();
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

describe("showing | hiding the right panel", () => {
  const side = (over: Partial<RightSide> = {}): RightSide => ({
    panel: null,
    hasOutline: true,
    outlineOn: true,
    hidden: false,
    ...over,
  });

  it("shows a plugin panel, else the outline when on; hidden shows nothing", () => {
    expect(rightShown(side())).toBe(true);
    expect(rightShown(side({ outlineOn: false }))).toBe(false);
    expect(rightShown(side({ outlineOn: false, panel: "live-feed.panel" }))).toBe(true);
    expect(rightShown(side({ hasOutline: false }))).toBe(false);
    expect(rightShown(side({ hidden: true }))).toBe(false);
    expect(rightShown(side({ hidden: true, panel: "live-feed.panel" }))).toBe(false);
    expect(rightAvailable(side({ hasOutline: false }))).toBe(false);
    expect(rightAvailable(side({ hasOutline: false, panel: "x" }))).toBe(true);
    expect(rightAvailable(side({ hidden: true, outlineOn: false }))).toBe(true);
  });

  it("the toggle hides what shows, shows what is hidden, and never does nothing visible", () => {
    const keep = { closePanel: false };
    expect(toggleRight(side())).toEqual({ hidden: true, outlineOn: true, ...keep });
    expect(toggleRight(side({ hidden: true }))).toEqual({
      hidden: false,
      outlineOn: true,
      ...keep,
    });
    // shown again with nothing on: the outline turns on
    expect(toggleRight(side({ outlineOn: false }))).toEqual({
      hidden: false,
      outlineOn: true,
      ...keep,
    });
    expect(toggleRight(side({ hidden: true, outlineOn: false }))).toEqual({
      hidden: false,
      outlineOn: true,
      ...keep,
    });
    // a plugin panel chosen: it shows again, the outline choice stays
    expect(toggleRight(side({ hidden: true, outlineOn: false, panel: "p" }))).toEqual({
      hidden: false,
      outlineOn: false,
      ...keep,
    });
    // nothing it could show: no change
    expect(toggleRight(side({ hasOutline: false, hidden: true }))).toEqual({
      hidden: true,
      outlineOn: true,
      ...keep,
    });
  });

  it("the outline toggle shows the outline unless it shows (a hidden panel comes back, a plugin panel lets go)", () => {
    expect(outlineShown(side())).toBe(true);
    expect(outlineShown(side({ panel: "p" }))).toBe(false);
    expect(outlineShown(side({ hidden: true }))).toBe(false);
    expect(toggleOutline(side())).toEqual({ hidden: false, outlineOn: false, closePanel: false });
    expect(toggleOutline(side({ hidden: true, outlineOn: false }))).toEqual({
      hidden: false,
      outlineOn: true,
      closePanel: true,
    });
    expect(toggleOutline(side({ hidden: true, outlineOn: true }))).toEqual({
      hidden: false,
      outlineOn: true,
      closePanel: true,
    });
    expect(toggleOutline(side({ outlineOn: false }))).toEqual({
      hidden: false,
      outlineOn: true,
      closePanel: true,
    });
    // a plugin panel shows (hidden or not): the outline replaces it
    for (const hidden of [false, true]) {
      expect(toggleOutline(side({ panel: "live-feed.panel", hidden }))).toEqual({
        hidden: false,
        outlineOn: true,
        closePanel: true,
      });
    }
  });

  it("rapid toggles alternate and end where an even | odd count says", () => {
    let now = side();
    for (let i = 0; i < 101; i++) now = { ...now, ...toggleRight(now) };
    expect(rightShown(now)).toBe(false);
    now = { ...now, ...toggleRight(now) };
    expect(rightShown(now)).toBe(true);
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
