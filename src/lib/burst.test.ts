import { describe, expect, it } from "vitest";
import { BurstTracker } from "./burst";

describe("BurstTracker", () => {
  it("counts distinct documents while changes keep arriving", () => {
    const burst = new BurstTracker();
    expect(burst.note("a.md", "modified", 0).count).toBe(1);
    expect(burst.note("b.md", "created", 3000).count).toBe(2);
    expect(burst.note("a.md", "modified", 6000).count).toBe(2);
    const last = burst.note("c.md", "modified", 9000);
    expect(last.count).toBe(3);
    expect(last.latest).toEqual({ path: "c.md", kind: "modified" });
  });

  it("starts a new burst after a pause longer than the window", () => {
    const burst = new BurstTracker(10_000);
    burst.note("a.md", "modified", 0);
    burst.note("b.md", "modified", 5000);
    expect(burst.note("c.md", "modified", 20_000).count).toBe(1);
  });

  it("starts over after reset", () => {
    const burst = new BurstTracker();
    burst.note("a.md", "modified", 0);
    burst.reset();
    expect(burst.note("b.md", "modified", 1000).count).toBe(1);
  });
});
