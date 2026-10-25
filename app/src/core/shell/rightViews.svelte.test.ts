// @vitest-environment jsdom
// The views' defaults (design-live-follow §14.1.3): decided once per project load, a yes at once, a no only after git
// had its time; the project's own choice always wins; the arrangement follows the settings and is written back.
import { flushSync } from "svelte";
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
const { contributions } = await import("../plugins/instance.svelte");
const { layout } = await import("./layout.svelte");
const { DEFAULT_WAIT_MS, rightViews } = await import("./rightViews.svelte");
const { defaultRightViews, LIVE_VIEW } = await import("./rightViews");

let calls = false;
const removals: (() => void)[] = [];
let stop: () => void = () => {};

beforeEach(async () => {
  vi.useFakeTimers();
  calls = false;
  removals.push(
    contributions.register({
      kind: "panel",
      id: LIVE_VIEW,
      owner: "live-feed" as PluginId,
      side: "right",
      label: "live.label",
      requires: NO_REQUIREMENT,
      view: () => import("../testing/StripFirst.svelte"),
      offered: () => true,
      defaultOpen: () => calls,
    }),
  );
  stop = rightViews.start(api);
  conditions.set({ ...neutralContext(), workspace: "project" });
  await api.projectSessionSet("layout.views", null);
  await layout.load(api, true);
  flushSync();
});

afterEach(async () => {
  stop();
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  await layout.load(api, false);
  vi.useRealTimers();
});

const live = () => {
  const panel = rightViews.panels.find((p) => p.id === LIVE_VIEW);
  if (panel === undefined) throw new Error("no panel");
  return panel;
};

describe("a view's default", () => {
  it("a yes while git may still answer is taken at once and kept for the load", () => {
    expect(rightViews.isOpen(live())).toBe(false);
    calls = true;
    expect(rightViews.isOpen(live())).toBe(true);
    calls = false;
    expect(rightViews.isOpen(live())).toBe(true);
  });

  it("a no is kept once git had its time: a late yes opens nothing this load, the next load decides", async () => {
    vi.advanceTimersByTime(DEFAULT_WAIT_MS + 10);
    flushSync();
    expect(rightViews.isOpen(live())).toBe(false);
    calls = true;
    expect(rightViews.isOpen(live())).toBe(false);
    await layout.load(api, true);
    flushSync();
    expect(rightViews.isOpen(live())).toBe(true);
  });

  it("the project's own choice wins over the default, both ways", () => {
    calls = true;
    layout.setOpen(LIVE_VIEW, false);
    expect(rightViews.isOpen(live())).toBe(false);
    calls = false;
    layout.setOpen(LIVE_VIEW, true);
    expect(rightViews.isOpen(live())).toBe(true);
  });

  it("outside a project: never open by default", () => {
    conditions.set({ ...neutralContext(), workspace: "notes" });
    calls = true;
    expect(rightViews.isOpen(live())).toBe(false);
  });
});

describe("the arrangement", () => {
  it("is drawn at once and written a moment after the last change", async () => {
    const tabs = { v: 1 as const, groups: [{ views: ["outline", LIVE_VIEW], share: 1 }] };
    rightViews.arrange(tabs);
    expect(rightViews.arrangement).toBe(tabs);
    await vi.advanceTimersByTimeAsync(450);
    const saved = await api.settingsGet();
    expect(saved.ok && (saved.value as unknown as { rightViews: unknown }).rightViews).toEqual(
      tabs,
    );
    rightViews.arrange(defaultRightViews());
    await vi.advanceTimersByTimeAsync(450);
  });
});
