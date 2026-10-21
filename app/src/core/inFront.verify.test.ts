// @vitest-environment jsdom
// Verifier (adversarial): the default wiring (`onScreen` = this view's tab vs its window's active tab) must be reactive
// inside an effect (unread | review timers re-run on a tab switch), and a view that moved to another window must not
// keep reading the old window's active tab.
import "./testing/media";
import { flushSync } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { InFront, onScreen } from "./inFront.svelte";
import { tabs } from "./shell/tabs.svelte";
import { observe } from "./testing/effect.svelte";
import { windowStore } from "./window.svelte";

const summary = (window: string, ids: number[], active: number | null) => ({
  window,
  closing: false,
  tabs: ids.map((tab) => ({
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

describe("verifier: on screen, default wiring", () => {
  it("an effect reading InFront re-runs on a tab switch (default onScreen, no injected function)", () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    tabs.window = "w-1";
    windowStore.tab = 2;
    tabs.windows = [summary("w-1", [1, 2], 2)];
    const front = new InFront();
    const offFront = front.start();
    const seen: boolean[] = [];
    const watch = observe(() => {
      seen.push(front.value);
    });
    flushSync();
    tabs.windows = [summary("w-1", [1, 2], 1)];
    flushSync();
    tabs.windows = [summary("w-1", [1, 2], 2)];
    flushSync();
    watch.stop();
    offFront();
    expect(seen).toEqual([true, false, true]);
  });

  it("a view moved to another window is not on screen through the old window's active tab", () => {
    tabs.window = "w-1";
    windowStore.tab = 2;
    tabs.windows = [summary("w-1", [1, 2], 2), summary("w-2", [3], 3)];
    expect(onScreen()).toBe(true);
    // view-moved: tab 2 now lives in w-2, behind tab 3; the old window still lists a stale active 2 for a moment
    tabs.window = "w-2";
    expect(onScreen()).toBe(false);
    tabs.windows = [summary("w-1", [1], 1), summary("w-2", [3, 2], 3)];
    expect(onScreen()).toBe(false);
    tabs.windows = [summary("w-1", [1], 1), summary("w-2", [3, 2], 2)];
    expect(onScreen()).toBe(true);
  });

  it("the tab list not read yet (boot, after a move): a tab view is not on screen; the Welcome view is", () => {
    tabs.window = "w-1";
    tabs.windows = [];
    windowStore.tab = 5;
    expect(onScreen()).toBe(false);
    windowStore.tab = null;
    expect(onScreen()).toBe(true);
  });
});
