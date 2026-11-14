// @vitest-environment jsdom
import "../testing/media";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { FILES, SEARCH } = await import("./arrangement");
const { arrangement } = await import("./arrangement.svelte");
const { layout } = await import("./layout.svelte");
const { offeredPanels } = await import("./panels.svelte");
const { showView } = await import("./sides");

const removals: (() => void)[] = [];
const view = () => import("../testing/StripFirst.svelte");

function panel(id: string, side: "left" | "right", owner = "test", offered?: () => boolean): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id,
      owner: owner as PluginId,
      side,
      label: "unread.title",
      requires: NO_REQUIREMENT,
      view,
      ...(offered ? { offered } : {}),
    }),
  );
}

const idsOf = (side: "left" | "right"): string[] =>
  offeredPanels()
    .filter((p) => p.side === side)
    .map((p) => p.id);

afterEach(() => {
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  void layout.load(api, false);
});

describe("plugin panels", () => {
  it("are listed with their side, and the tab that shows falls back to Files when its plugin is gone", () => {
    conditions.set({ ...neutralContext(), workspace: "project" });
    panel("a.panel", "left");
    panel("b.panel", "right");
    expect(idsOf("left")).toEqual(["a.panel"]);
    expect(idsOf("right")).toEqual(["b.panel"]);
    const shown = (): string[] =>
      [FILES, SEARCH, "a.panel"].filter((id) => arrangement.isSelected(id));
    expect(shown()).toEqual([FILES]);
    showView("a.panel");
    expect(shown()).toEqual(["a.panel"]);
    showView(SEARCH);
    expect(shown()).toEqual([SEARCH]);
    showView("a.panel");
    // its plugin goes off: the first view of the group shows again
    for (const remove of removals.splice(0)) remove();
    expect(shown()).toEqual([FILES]);
    // a view nobody offers selects nothing
    showView("gone.panel");
    expect(shown()).toEqual([FILES]);
  });

  it("a dormant panel is not listed until its plugin offers it", () => {
    const state = { offered: false };
    panel("b.panel", "right", "test", () => state.offered);
    panel("c.panel", "left", "test", () => state.offered);
    expect(idsOf("right")).toEqual([]);
    expect(idsOf("left")).toEqual([]);
    state.offered = true;
    expect(idsOf("right")).toEqual(["b.panel"]);
    expect(idsOf("left")).toEqual(["c.panel"]);
  });

  it("keep one order whatever order the plugins finished loading in", () => {
    panel("unread.panel", "left", "unread");
    panel("tags.panel", "left", "tags");
    panel("broken.panel", "left", "broken-links");
    panel("tags.second", "left", "tags");
    expect(idsOf("left")).toEqual(["broken.panel", "tags.panel", "tags.second", "unread.panel"]);
    for (const remove of removals.splice(0)) remove();
    panel("broken.panel", "left", "broken-links");
    panel("tags.panel", "left", "tags");
    panel("tags.second", "left", "tags");
    panel("unread.panel", "left", "unread");
    expect(idsOf("left")).toEqual(["broken.panel", "tags.panel", "tags.second", "unread.panel"]);
  });
});
