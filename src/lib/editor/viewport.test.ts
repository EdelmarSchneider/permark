// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { watchViewport } from "./viewport";

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
