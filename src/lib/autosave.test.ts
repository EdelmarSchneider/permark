import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AutosaveTimer, Autosaver, MAX_WAIT_MS, QUIET_MS } from "./autosave";

describe("AutosaveTimer (instants are given, nothing is read from a clock)", () => {
  it("is due 750 ms after the last edit", () => {
    const timer = new AutosaveTimer();
    expect(timer.pending).toBe(false);
    expect(timer.deadline()).toBeNull();
    expect(timer.due(10_000)).toBe(false);
    timer.touch(1000);
    expect(timer.pending).toBe(true);
    expect(timer.deadline()).toBe(1000 + QUIET_MS);
    expect(timer.due(1000 + QUIET_MS - 1)).toBe(false);
    expect(timer.due(1000 + QUIET_MS)).toBe(true);
  });

  it("keeps waiting while typing goes on, but never longer than 3 s from the first edit", () => {
    const timer = new AutosaveTimer();
    for (let at = 0; at <= 2800; at += 200) timer.touch(at);
    // the quiet deadline moved with every key; the cap did not
    expect(timer.deadline()).toBe(MAX_WAIT_MS);
    expect(timer.due(MAX_WAIT_MS - 1)).toBe(false);
    expect(timer.due(MAX_WAIT_MS)).toBe(true);
    timer.reset();
    expect(timer.pending).toBe(false);
    // after a save, the next edit starts a new wait
    timer.touch(5000);
    expect(timer.deadline()).toBe(5000 + QUIET_MS);
  });
});

describe("Autosaver", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const make = (work: () => Promise<unknown> = async () => {}) => {
    const save = vi.fn(work);
    return { save, autosaver: new Autosaver({ save }) };
  };

  it("saves once, 750 ms after typing stops", async () => {
    const { save, autosaver } = make();
    autosaver.touch();
    await vi.advanceTimersByTimeAsync(QUIET_MS - 1);
    expect(save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(save).toHaveBeenCalledTimes(1);
    expect(autosaver.busy).toBe(false);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("saves at least every 3 s while typing goes on", async () => {
    const { save, autosaver } = make();
    for (let i = 0; i < 20; i++) {
      autosaver.touch();
      await vi.advanceTimersByTimeAsync(200);
    }
    // 4 s of typing, a key every 200 ms: one save at 3 s
    expect(save).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(QUIET_MS);
    expect(save).toHaveBeenCalledTimes(2);
  });

  it("flush saves at once and does nothing when there is nothing to save", async () => {
    const { save, autosaver } = make();
    await autosaver.flush();
    expect(save).not.toHaveBeenCalled();
    autosaver.touch();
    await autosaver.flush();
    expect(save).toHaveBeenCalledTimes(1);
    // the timer was cancelled: no second save later
    await vi.advanceTimersByTimeAsync(10_000);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("never runs two saves at once, and saves what was typed while one ran", async () => {
    let finish: () => void = () => {};
    let running = 0;
    let most = 0;
    const { save, autosaver } = make(
      () =>
        new Promise<void>((resolve) => {
          running++;
          most = Math.max(most, running);
          finish = () => {
            running--;
            resolve();
          };
        }),
    );
    autosaver.touch();
    await vi.advanceTimersByTimeAsync(QUIET_MS);
    expect(save).toHaveBeenCalledTimes(1);
    // typing while the save runs
    autosaver.touch();
    await vi.advanceTimersByTimeAsync(5000);
    expect(save).toHaveBeenCalledTimes(1);
    finish();
    await vi.advanceTimersByTimeAsync(QUIET_MS);
    expect(save).toHaveBeenCalledTimes(2);
    finish();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(most).toBe(1);
    expect(save).toHaveBeenCalledTimes(2);
  });

  it("flush waits for a save that is running and then saves the rest", async () => {
    const order: string[] = [];
    let finish: () => void = () => {};
    let calls = 0;
    const { autosaver } = make(async () => {
      const call = ++calls;
      order.push(`start ${call}`);
      if (call === 1) await new Promise<void>((resolve) => (finish = resolve));
      order.push(`end ${call}`);
    });
    autosaver.touch();
    await vi.advanceTimersByTimeAsync(QUIET_MS);
    autosaver.touch();
    const flushed = autosaver.flush().then(() => order.push("flushed"));
    await vi.advanceTimersByTimeAsync(0);
    expect(order).toEqual(["start 1"]);
    finish();
    await flushed;
    expect(order).toEqual(["start 1", "end 1", "start 2", "end 2", "flushed"]);
    expect(autosaver.busy).toBe(false);
  });

  it("cancel forgets what was waiting", async () => {
    const { save, autosaver } = make();
    autosaver.touch();
    autosaver.cancel();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(save).not.toHaveBeenCalled();
    expect(autosaver.busy).toBe(false);
  });

  it("goes on after a save that failed", async () => {
    let calls = 0;
    const { save, autosaver } = make(async () => {
      calls++;
      if (calls === 1) throw new Error("disk full");
    });
    autosaver.touch();
    await expect(autosaver.flush()).rejects.toThrow("disk full");
    expect(autosaver.busy).toBe(false);
    autosaver.touch();
    await autosaver.flush();
    expect(save).toHaveBeenCalledTimes(2);
  });
});
