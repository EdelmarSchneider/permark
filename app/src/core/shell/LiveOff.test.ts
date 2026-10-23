// @vitest-environment jsdom
// "Live updates off" in the status bar (UX regressions #6): while a project's folder cannot report changes, and
// only then; never on the Welcome screen, a single file or the Notes.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { Context } from "../../api";
import { conditions, neutralContext } from "../conditions/context.svelte";
import LiveOff from "./LiveOff.svelte";
import { liveOff } from "./liveSignal.svelte";

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

const HINT =
  "This location can't report changes, so PerMark can't follow them live. Switch back to the window to refresh.";

describe("live updates off", () => {
  it("shows an icon and a short label, named in words for screen readers", () => {
    const el = show({ workspace: "project", liveUpdates: off });
    const item = el.querySelector<HTMLButtonElement>("button.item");
    expect(item?.textContent?.trim()).toBe("Live: off");
    expect(item?.getAttribute("aria-label")).toBe("Live updates off");
    expect(item?.querySelector("svg")).not.toBeNull();
    expect(item?.getAttribute("title")).toBe(HINT);
    expect(el.querySelector('[role="status"]')?.textContent).toBe(`Live updates off. ${HINT}`);
  });

  it("opens the why and what to do by keyboard, Esc closes (UX review restore-b4 F4)", () => {
    const el = show({ workspace: "project", liveUpdates: off });
    const item = el.querySelector<HTMLButtonElement>("button.item");
    item?.focus();
    item?.click();
    flushSync();
    expect(item?.getAttribute("aria-expanded")).toBe("true");
    expect(el.querySelector("#live-off-hint")?.textContent?.trim()).toBe(HINT);
    item?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(el.querySelector("#live-off-hint")).toBeNull();
    expect(document.activeElement).toBe(item);
  });

  it("steps aside while the Live chip shows the off state; screen readers still hear it once (F3)", () => {
    const el = show({ workspace: "project", liveUpdates: off });
    const release = liveOff.claim();
    flushSync();
    expect(el.querySelector("button.item")).toBeNull();
    expect(el.querySelector('[role="status"]')?.textContent).toContain("Live updates off");
    release();
    release();
    flushSync();
    expect(el.querySelector("button.item")).not.toBeNull();
  });

  it("goes when live updates work again, and the live region stays to say it next time", () => {
    const el = show({ workspace: "project", liveUpdates: off });
    expect(el.querySelector(".item")).not.toBeNull();
    show({ workspace: "project", liveUpdates: { status: "available" } });
    expect(el.querySelector(".item")).toBeNull();
    expect(el.querySelector('[role="status"]')?.textContent).toBe("");
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
