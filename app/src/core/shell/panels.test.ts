// @vitest-environment jsdom
import "../testing/media";
import { afterEach, describe, expect, it } from "vitest";
import { contributions } from "../plugins/instance.svelte";
import { NO_REQUIREMENT } from "../conditions/available";
import { activeRight, activeSidebar, leftPanels, rightPanels } from "./panels.svelte";
import { ui } from "./ui.svelte";

const removals: (() => void)[] = [];
const view = () => import("../testing/StripFirst.svelte");

function panel(id: string, side: "left" | "right"): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id,
      owner: "test",
      side,
      label: "unread.title",
      requires: NO_REQUIREMENT,
      view,
    }),
  );
}

afterEach(() => {
  for (const remove of removals.splice(0)) remove();
  ui.sidebar = "files";
  ui.right = null;
});

describe("plugin panels", () => {
  it("are listed by side, and the view that shows falls back to Files when its plugin is gone", () => {
    panel("a.panel", "left");
    panel("b.panel", "right");
    expect(leftPanels().map((p) => p.id)).toEqual(["a.panel"]);
    expect(rightPanels().map((p) => p.id)).toEqual(["b.panel"]);
    ui.showSidebar("a.panel");
    expect(activeSidebar()).toBe("a.panel");
    ui.showSidebar("search");
    expect(activeSidebar()).toBe("search");
    ui.showSidebar("gone.panel");
    expect(activeSidebar()).toBe("files");
  });

  it("the right panel shows only when chosen and still there, and toggles", () => {
    expect(activeRight()).toBeNull();
    panel("b.panel", "right");
    ui.toggleRight("b.panel");
    expect(activeRight()).toBe("b.panel");
    ui.toggleRight("b.panel");
    expect(activeRight()).toBeNull();
    ui.right = "missing.panel";
    expect(activeRight()).toBeNull();
  });
});
