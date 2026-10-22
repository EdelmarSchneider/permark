// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReviewView } from "../../api";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { review } = await import("./store.svelte");
const { default: ReviewBar } = await import("./ReviewBar.svelte");

const VIEW: ReviewView = {
  path: "docs/guide.md",
  hash: "ab".repeat(8),
  basis: "read",
  asked: "read",
  unit: "blocks",
  count: 2,
  canKeep: true,
  changes: [],
};

let target: HTMLElement;
let tree: HTMLElement;
let component: Record<string, unknown> | null = null;

beforeEach(() => {
  document.body.replaceChildren();
  tree = document.createElement("div");
  tree.setAttribute("role", "tree");
  tree.tabIndex = 0;
  const area = document.createElement("section");
  area.className = "area";
  target = document.createElement("div");
  const scroller = document.createElement("div");
  scroller.dataset["scroller"] = "";
  scroller.tabIndex = -1;
  area.append(target, scroller);
  document.body.append(tree, area);
  review.state = { status: "idle" };
  review.basis = "read";
  component = mount(ReviewBar, { target });
  flushSync();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  review.state = { status: "idle" };
  vi.restoreAllMocks();
});

const radios = (): HTMLElement[] =>
  Array.from(target.querySelectorAll<HTMLElement>('[role="radio"]'));

function key(el: HTMLElement, name: string, init: KeyboardEventInit = {}): void {
  el.dispatchEvent(
    new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...init }),
  );
  flushSync();
}

describe("the review bar", () => {
  it("opened from the palette (the keyboard went back to the file list): the keyboard goes to the bar", () => {
    tree.focus();
    review.state = { status: "reviewing", view: VIEW, at: 0 };
    flushSync();
    expect(target.querySelector(".bar")?.contains(document.activeElement)).toBe(true);
  });

  it("Compare with = one Tab stop; arrows move the choice and the keyboard with it", async () => {
    review.state = { status: "reviewing", view: VIEW, at: 0 };
    flushSync();
    expect(radios().map((r) => r.tabIndex)).toEqual([0, -1]);
    const set = vi.spyOn(review, "setBasis").mockResolvedValue();
    radios()[0]!.focus();
    key(radios()[0]!, "ArrowRight");
    await tick();
    expect(set).toHaveBeenCalledWith("commit");
    expect(document.activeElement).toBe(radios()[1]);
    // a modified arrow is not a move (Alt+Left = back)
    key(radios()[1]!, "ArrowLeft", { altKey: true });
    expect(set).toHaveBeenCalledTimes(1);
    key(radios()[1]!, "ArrowLeft");
    expect(set).toHaveBeenLastCalledWith("read");
  });
});
