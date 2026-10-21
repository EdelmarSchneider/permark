// @vitest-environment jsdom
import "./testing/media";
import { flushSync } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { InFront, onScreen, viewShown } from "./inFront.svelte";
import { tabs } from "./shell/tabs.svelte";
import { windowStore } from "./window.svelte";

const summary = (active: number | null) => ({
  window: "w-1",
  closing: false,
  tabs: [1, 2].map((tab) => ({
    tab,
    kind: "project" as const,
    title: `p${tab}`,
    path: `C:/p${tab}`,
    loaded: true,
    unsaved: false,
  })),
  active,
  focusedMs: 0,
});

afterEach(() => {
  vi.restoreAllMocks();
  tabs.windows = [];
  tabs.window = "";
  windowStore.tab = null;
});

describe("which view is on screen", () => {
  it("is the window's Welcome, or the tab that is active", () => {
    expect(viewShown(null, null)).toBe(true);
    expect(viewShown(2, 2)).toBe(true);
    expect(viewShown(2, 1)).toBe(false);
    // the tab list not read yet: no tab is known to be on screen
    expect(viewShown(2, null)).toBe(false);
  });

  it("follows this view's tab and its window's active tab", () => {
    tabs.window = "w-1";
    windowStore.tab = 2;
    tabs.windows = [summary(1)];
    expect(onScreen()).toBe(false);
    tabs.windows = [summary(2)];
    expect(onScreen()).toBe(true);
  });
});

describe("in front", () => {
  it("needs the focus and the view on screen: a hidden tab with the focus is not in front", () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    let shown = true;
    const front = new InFront(() => shown);
    const stop = front.start();
    expect(front.value).toBe(true);
    shown = false;
    expect(front.value).toBe(false);
    shown = true;
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    window.dispatchEvent(new Event("blur"));
    flushSync();
    expect(front.value).toBe(false);
    stop();
  });
});
