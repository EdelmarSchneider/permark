import { describe, expect, it } from "vitest";
import type { WindowView, WorkspaceSummary } from "../../api";
import { barShows, neighbor, shownId } from "./switcherModel";

function w(id: number, kind: WorkspaceSummary["kind"] = "project"): WorkspaceSummary {
  return { id, kind, title: `w${id}`, path: `/w${id}`, windows: 1 };
}

describe("the switcher bar", () => {
  it("shows with two or more workspaces and never with fewer", () => {
    expect(barShows([])).toBe(false);
    expect(barShows([w(1)])).toBe(false);
    expect(barShows([w(1), w(2)])).toBe(true);
  });

  it("next | previous wrap around the list", () => {
    const list = [w(1), w(2), w(3)];
    expect(neighbor(list, 1, 1)?.id).toBe(2);
    expect(neighbor(list, 3, 1)?.id).toBe(1);
    expect(neighbor(list, 1, -1)?.id).toBe(3);
    expect(neighbor(list, 2, -1)?.id).toBe(1);
  });

  it("a window that shows nothing goes to the first (next) or the last (previous)", () => {
    const list = [w(1), w(2), w(3)];
    expect(neighbor(list, null, 1)?.id).toBe(1);
    expect(neighbor(list, null, -1)?.id).toBe(3);
    // a workspace that is not in the list (just closed) is the same as none
    expect(neighbor(list, 99, 1)?.id).toBe(1);
  });

  it("with one workspace or none there is nowhere to go", () => {
    expect(neighbor([], null, 1)).toBeNull();
    expect(neighbor([w(1)], 1, 1)).toBeNull();
  });

  it("the workspace a window shows, also while another opens", () => {
    const project = {
      kind: "project",
      id: 5,
      name: "p",
      root: "/p",
      stateLocation: "profile",
    } as const;
    const showing: WindowView = { state: "showing", workspace: project, document: null };
    expect(shownId(showing)).toBe(5);
    const target = { kind: "notes", path: null } as const;
    expect(shownId({ state: "opening", target, showing: project })).toBe(5);
    expect(shownId({ state: "opening", target, showing: null })).toBeNull();
    expect(shownId({ state: "welcome" })).toBeNull();
    expect(shownId({ state: "closing" })).toBeNull();
  });
});
