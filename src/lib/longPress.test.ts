// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LONG_PRESS_MS, LongPress } from "./longPress";

const button = document.createElement("button");
const press = (init: { button?: number; currentTarget?: EventTarget | null } = {}) => ({
  button: init.button ?? 0,
  currentTarget: init.currentTarget === undefined ? button : init.currentTarget,
});

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("LongPress", () => {
  it("runs after the delay, with the control that was pressed", () => {
    const onLong = vi.fn();
    const lp = new LongPress(onLong);
    lp.down(press());
    vi.advanceTimersByTime(LONG_PRESS_MS - 1);
    expect(onLong).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onLong).toHaveBeenCalledTimes(1);
    expect(onLong).toHaveBeenCalledWith(button);
  });

  it("is a half second, as a browser's long press is", () => {
    expect(LONG_PRESS_MS).toBe(500);
  });

  it("never runs for a press that ended first", () => {
    const onLong = vi.fn();
    const lp = new LongPress(onLong);
    lp.down(press());
    vi.advanceTimersByTime(200);
    lp.release();
    vi.advanceTimersByTime(1000);
    expect(onLong).not.toHaveBeenCalled();
    expect(lp.swallowClick()).toBe(false);
  });

  it("never runs when the pointer leaves the control or the gesture is taken over", () => {
    const onLong = vi.fn();
    const lp = new LongPress(onLong);
    lp.down(press());
    vi.advanceTimersByTime(300);
    lp.cancel();
    vi.advanceTimersByTime(1000);
    expect(onLong).not.toHaveBeenCalled();
  });

  it("starts only on the main button", () => {
    const onLong = vi.fn();
    const lp = new LongPress(onLong);
    lp.down(press({ button: 2 }));
    lp.down(press({ button: 1 }));
    vi.advanceTimersByTime(1000);
    expect(onLong).not.toHaveBeenCalled();
  });

  it("ignores a press that is not on an element", () => {
    const onLong = vi.fn();
    const lp = new LongPress(onLong);
    lp.down(press({ currentTarget: null }));
    vi.advanceTimersByTime(1000);
    expect(onLong).not.toHaveBeenCalled();
  });

  it("swallows the click that ends a long press, once", () => {
    const lp = new LongPress(() => {});
    lp.down(press());
    vi.advanceTimersByTime(LONG_PRESS_MS);
    lp.release();
    expect(lp.swallowClick()).toBe(true);
    expect(lp.swallowClick()).toBe(false);
  });

  it("does not swallow a later click when the long press ended with no click (released elsewhere)", () => {
    const lp = new LongPress(() => {});
    lp.down(press());
    vi.advanceTimersByTime(LONG_PRESS_MS);
    lp.release();
    // no click arrived; a click made with the keyboard a moment later is a real one
    vi.advanceTimersByTime(1);
    expect(lp.swallowClick()).toBe(false);
  });

  it("starts over with each press", () => {
    const onLong = vi.fn();
    const lp = new LongPress(onLong);
    lp.down(press());
    vi.advanceTimersByTime(400);
    lp.down(press());
    vi.advanceTimersByTime(400);
    expect(onLong).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    expect(onLong).toHaveBeenCalledOnce();
  });
});
