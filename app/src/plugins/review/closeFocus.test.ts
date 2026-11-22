// @vitest-environment jsdom
// A review that ends under the keyboard (its last change kept | reverted, Done) leaves it on the page it was read in,
// with no focus ring flashing around the page: the person never left it.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ReviewView } from "../../api";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { review } = await import("./store.svelte");
const { SENT } = await import("../../core/doc/paneFocus");
const { default: ReviewBar } = await import("./ReviewBar.svelte");

const VIEW: ReviewView = {
  path: "docs/guide.md",
  hash: "ab".repeat(8),
  basis: "read",
  asked: "read",
  unit: "blocks",
  count: 1,
  canKeep: true,
  uncommitted: false,
  changes: [],
};

let scroller: HTMLElement;
let component: Record<string, unknown> | null = null;

beforeEach(() => {
  const area = document.createElement("section");
  area.className = "area";
  const target = document.createElement("div");
  scroller = document.createElement("div");
  scroller.dataset["scroller"] = "";
  scroller.tabIndex = -1;
  area.append(target, scroller);
  document.body.append(area);
  review.state = { status: "idle" };
  component = mount(ReviewBar, { target });
  flushSync();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  review.state = { status: "idle" };
  document.body.replaceChildren();
});

it("ended with the keyboard on its bar: the page's pane holds the keyboard, not marked to draw a ring", () => {
  review.state = { status: "reviewing", view: VIEW, at: 0 };
  flushSync();
  expect(document.activeElement?.closest(".bar")).not.toBeNull();
  review.state = { status: "idle" };
  flushSync();
  expect(document.activeElement).toBe(scroller);
  expect(scroller.hasAttribute(SENT)).toBe(false);
});
