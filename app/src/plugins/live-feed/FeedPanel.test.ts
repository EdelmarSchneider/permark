// @vitest-environment jsdom
// The Changes panel's keyboard: the All | Mine | Outside filter is a radio group the arrows move (one stop in the Tab
// order), and Clear wears an icon that says it empties the list.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { default: FeedPanel } = await import("./FeedPanel.svelte");
const { feed } = await import("./store.svelte");

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  feed.show = "all";
});

function draw(): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(FeedPanel, { target });
  flushSync();
  return target;
}

function radios(target: HTMLElement): HTMLElement[] {
  const group = target.querySelector('[role="radiogroup"]');
  if (group === null) throw new Error("no filter group");
  return [...group.querySelectorAll<HTMLElement>('[role="radio"]')];
}

function press(el: Element, key: string, init: KeyboardEventInit = {}): void {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
  flushSync();
}

describe("the Changes filter", () => {
  it("is one Tab stop: only the checked choice is in the Tab order", () => {
    const target = draw();
    const group = target.querySelector('[role="radiogroup"]');
    expect(group?.getAttribute("aria-label")).toBe("Show");
    expect(radios(target).map((r) => r.getAttribute("aria-checked"))).toEqual([
      "true",
      "false",
      "false",
    ]);
    expect(radios(target).map((r) => r.tabIndex)).toEqual([0, -1, -1]);
  });

  it("moves the check and the focus with the arrows, both ways and around", async () => {
    const target = draw();
    radios(target)[0]?.focus();
    press(radios(target)[0]!, "ArrowRight");
    expect(feed.show).toBe("mine");
    await vi.waitFor(() => expect(document.activeElement).toBe(radios(target)[1]));
    expect(radios(target).map((r) => r.getAttribute("aria-checked"))).toEqual([
      "false",
      "true",
      "false",
    ]);
    expect(radios(target).map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
    press(radios(target)[1]!, "ArrowLeft");
    press(radios(target)[0]!, "ArrowLeft");
    expect(feed.show).toBe("outside");
    await vi.waitFor(() => expect(document.activeElement).toBe(radios(target)[2]));
  });
});

describe("the Clear button", () => {
  it("wears the eraser, not the refresh arrows (it empties the list, it does not reload it)", () => {
    const target = draw();
    const clear = target.querySelector('button[aria-label="Clear the list"]');
    expect(clear?.querySelector(".lucide-eraser")).not.toBeNull();
    expect(clear?.querySelector(".lucide-refresh-cw")).toBeNull();
  });
});
