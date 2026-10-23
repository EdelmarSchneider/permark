// @vitest-environment jsdom
// Live updates off, said in one place (UX review restore-b4 F3 + F4): the Live chip shows the off state and claims
// the signal (the core's own item steps aside); the Changes panel it opens says why and what to do.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { conditions, neutralContext } = await import("../../core/conditions/context.svelte");
const { liveOff } = await import("../../core/shell/liveSignal.svelte");
const { default: FeedStatus } = await import("./FeedStatus.svelte");
const { default: LiveOffNote } = await import("../../core/shell/LiveOffNote.svelte");
const { feed } = await import("./store.svelte");

const off = { status: "unavailable", reason: "watchFailed" } as const;
let mounted: Record<string, unknown>[] = [];

function draw(component: typeof FeedStatus | typeof LiveOffNote): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  mounted.push(mount(component, { target }));
  flushSync();
  return target;
}

afterEach(() => {
  for (const m of mounted) void unmount(m);
  mounted = [];
  document.body.replaceChildren();
  feed.offered = false;
  conditions.set(neutralContext());
});

describe("the Live chip when live updates are off", () => {
  it("shows the off state itself and claims the signal; back to Live when they work again", () => {
    feed.offered = true;
    conditions.set({ ...neutralContext(), workspace: "project", liveUpdates: off });
    const el = draw(FeedStatus);
    const chip = el.querySelector<HTMLButtonElement>("button.chip");
    expect(chip?.textContent?.trim()).toBe("Live: off");
    expect(chip?.classList.contains("off")).toBe(true);
    expect(chip?.getAttribute("title")).toContain("can't report changes");
    expect(liveOff.claimed).toBe(true);
    conditions.set({
      ...neutralContext(),
      workspace: "project",
      liveUpdates: { status: "available" },
    });
    flushSync();
    expect(chip?.textContent?.trim()).toBe("Live");
    expect(liveOff.claimed).toBe(false);
  });

  it("does not claim while the chip is dormant (the core's item says it then)", () => {
    conditions.set({ ...neutralContext(), workspace: "project", liveUpdates: off });
    const el = draw(FeedStatus);
    expect(el.querySelector("button.chip")).toBeNull();
    expect(liveOff.claimed).toBe(false);
  });

  it("the panel's note says why and what to do, only while live updates are off", () => {
    conditions.set({ ...neutralContext(), workspace: "project", liveUpdates: off });
    const el = draw(LiveOffNote);
    expect(el.textContent).toContain("Live updates off.");
    expect(el.textContent).toContain("Switch back to the window to refresh.");
    conditions.set({
      ...neutralContext(),
      workspace: "project",
      liveUpdates: { status: "available" },
    });
    flushSync();
    expect(el.textContent?.trim()).toBe("");
  });
});
