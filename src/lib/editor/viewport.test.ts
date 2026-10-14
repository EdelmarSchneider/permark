// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { showsAtTop, watchViewport } from "./viewport";

describe("watchViewport", () => {
  it("tells scrolling from the person taking over", () => {
    const el = document.createElement("div");
    const seen: boolean[] = [];
    watchViewport(el, (byUser) => seen.push(byUser));
    el.dispatchEvent(new Event("scroll"));
    el.dispatchEvent(new Event("wheel"));
    el.dispatchEvent(new Event("touchmove"));
    el.dispatchEvent(new Event("keydown", { bubbles: true }));
    el.dispatchEvent(new Event("pointerdown"));
    expect(seen).toEqual([false, true, true, true, true]);
  });

  it("hears a key typed in a child, which bubbles up to the scroller", () => {
    const el = document.createElement("div");
    const child = el.appendChild(document.createElement("div"));
    const notify = vi.fn();
    watchViewport(el, notify);
    child.dispatchEvent(new Event("keydown", { bubbles: true }));
    expect(notify).toHaveBeenCalledWith(true);
  });

  it("stops when asked", () => {
    const el = document.createElement("div");
    const notify = vi.fn();
    const stop = watchViewport(el, notify);
    stop();
    el.dispatchEvent(new Event("scroll"));
    el.dispatchEvent(new Event("wheel"));
    expect(notify).not.toHaveBeenCalled();
  });
});

describe("showsAtTop", () => {
  it("is the block that has a line's worth of itself in view at the top edge", () => {
    // the edge is at 100; a heading 34 px tall with 12 px showing was scrolled past, a paragraph with 60 px showing is the one at the top
    expect(showsAtTop(78, 112, 100)).toBe(false);
    expect(showsAtTop(60, 160, 100)).toBe(true);
    expect(showsAtTop(98, 160, 100)).toBe(true);
  });

  it("takes a short block that shows in full, or by more than half", () => {
    expect(showsAtTop(100, 120, 100)).toBe(true);
    expect(showsAtTop(90, 110, 100)).toBe(true);
    expect(showsAtTop(80, 120, 100)).toBe(true);
    expect(showsAtTop(90, 120, 100)).toBe(true);
  });

  it("takes a block that is further down, and leaves one that is above the edge or has no height", () => {
    expect(showsAtTop(300, 340, 100)).toBe(true);
    expect(showsAtTop(0, 90, 100)).toBe(false);
    expect(showsAtTop(100, 100, 100)).toBe(false);
    expect(showsAtTop(0, 0, 100)).toBe(false);
  });
});
