import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import { clampWidth, fit, keyWidth, PAGE_MIN, RIGHT, SIDEBAR } from "./layout";
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
