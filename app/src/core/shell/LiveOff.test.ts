// @vitest-environment jsdom
// "Live updates off" in the status bar (UX regressions #6): while a project's folder cannot report changes, and
// only then; never on the Welcome screen, a single file or the Notes.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { Context } from "../../api";
import { conditions, neutralContext } from "../conditions/context.svelte";
import LiveOff from "./LiveOff.svelte";

let component: Record<string, unknown> | null = null;
let target: HTMLElement;

function show(over: Partial<Context>): HTMLElement {
  conditions.set({ ...neutralContext(), ...over });
  if (component === null) {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(LiveOff, { target });
  }
  flushSync();
  return target;
}

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.remove();
  conditions.set(neutralContext());
});

const off = { status: "unavailable", reason: "watchFailed" } as const;

describe("live updates off", () => {
  it("shows an icon and words, with the reason in the tooltip and for screen readers", () => {
    const el = show({ workspace: "project", liveUpdates: off });
    const item = el.querySelector(".item");
    expect(item?.textContent).toContain("Live updates off");
    expect(item?.querySelector("svg")).not.toBeNull();
    expect(item?.getAttribute("title")).toBe(
      "This location can't report changes, so PerMark can't follow them live. Switch back to the window to refresh.",
    );
    expect(el.querySelector('[role="status"] .sr-only')?.textContent).toContain(
      "can't report changes",
    );
  });

  it("goes when live updates work again, and the live region stays to say it next time", () => {
    const el = show({ workspace: "project", liveUpdates: off });
    expect(el.querySelector(".item")).not.toBeNull();
    show({ workspace: "project", liveUpdates: { status: "available" } });
    expect(el.querySelector(".item")).toBeNull();
    expect(el.querySelector('[role="status"]')).not.toBeNull();
  });

  it("never shows where nothing follows live: Welcome, a single file, the Notes", () => {
    expect(show({}).querySelector(".item")).toBeNull();
    const noWatcher = { status: "unavailable", reason: "noWatcher" } as const;
    expect(
      show({ workspace: "singleFile", liveUpdates: noWatcher }).querySelector(".item"),
    ).toBeNull();
    expect(show({ workspace: "notes", liveUpdates: noWatcher }).querySelector(".item")).toBeNull();
  });
});
