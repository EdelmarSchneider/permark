import { describe, expect, it } from "vitest";
import { EdgeScroll, type FrameClock } from "./edgeScroll";

/** A list 400 px high at the top of the view, and a frame clock the test steps. */
function fx(): { list: HTMLElement; clock: FrameClock; step: () => void; pending: () => number } {
  const list = {
    scrollTop: 100,
    getBoundingClientRect: () => ({ top: 0, bottom: 400 }),
  } as unknown as HTMLElement;
  let queued: (() => void)[] = [];
  let next = 1;
  const ids = new Map<number, () => void>();
  const clock: FrameClock = {
    request: (run) => {
      const id = next++;
      ids.set(id, run);
      queued.push(run);
      return id;
    },
    cancel: (id) => {
      const run = ids.get(id);
      queued = queued.filter((q) => q !== run);
    },
  };
  return {
    list,
    clock,
    step: () => {
      const now = queued;
      queued = [];
      for (const run of now) run();
    },
    pending: () => queued.length,
  };
}

describe("edge scroll", () => {
  it("scrolls each frame while the pointer stays near an edge, then stops on its own", () => {
    const f = fx();
    const scroll = new EdgeScroll(f.list, f.clock);
    scroll.point(0);
    f.step();
    f.step();
    expect(f.list.scrollTop).toBe(72);
    scroll.point(200);
    f.step();
    expect(f.pending()).toBe(0);
    expect(f.list.scrollTop).toBe(72);
  });

  it("one loop however many points arrive; down near the bottom", () => {
    const f = fx();
    const scroll = new EdgeScroll(f.list, f.clock);
    for (let n = 0; n < 50; n++) scroll.point(400);
    expect(f.pending()).toBe(1);
    f.step();
    expect(f.list.scrollTop).toBe(114);
  });

  it("the end of the drag stops it at once (teardown)", () => {
    const f = fx();
    const scroll = new EdgeScroll(f.list, f.clock);
    scroll.point(0);
    scroll.point(null);
    expect(f.pending()).toBe(0);
    scroll.point(0);
    scroll.stop();
    f.step();
    expect(f.list.scrollTop).toBe(100);
  });

  it("a point in the middle starts nothing", () => {
    const f = fx();
    new EdgeScroll(f.list, f.clock).point(200);
    expect(f.pending()).toBe(0);
  });
});
