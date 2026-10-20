import { describe, expect, it } from "vitest";
import { NO_REQUIREMENT } from "../conditions/available";
import type { ContributionOf } from "../plugins/registry.svelte";
import { stepChange } from "./changes";

type Step = ContributionOf<"changeStep">;

function step(order: number, run: Step["step"]): Step {
  return {
    kind: "changeStep",
    owner: "review",
    id: `s${order}`,
    order,
    requires: NO_REQUIREMENT,
    step: run,
  };
}

const registry = (...steps: Step[]) => ({ of: () => steps }) as never;

describe("stepChange", () => {
  it("runs the steps lowest order first and stops at the first that moved", async () => {
    const seen: string[] = [];
    const late = step(20, () => (seen.push("late"), true));
    const early = step(10, () => (seen.push("early"), true));
    expect(await stepChange(registry(late, early), 1)).toBe(true);
    expect(seen).toEqual(["early"]);
  });

  it("passes on to the next step when one has nothing to move to, and says false when none has", async () => {
    const seen: number[] = [];
    const quiet = step(10, () => (seen.push(10), false));
    const slow = step(20, async () => {
      seen.push(20);
      return true;
    });
    expect(await stepChange(registry(quiet, slow), -1)).toBe(true);
    expect(seen).toEqual([10, 20]);
    expect(await stepChange(registry(quiet), 1)).toBe(false);
    expect(await stepChange(registry(), 1)).toBe(false);
  });

  it("hands the direction on and skips a step that throws or rejects", async () => {
    const directions: number[] = [];
    const broken = step(10, () => {
      throw new Error("no");
    });
    const rejected = step(15, () => Promise.reject(new Error("no")));
    const good = step(20, (direction) => (directions.push(direction), true));
    expect(await stepChange(registry(broken, rejected, good), -1)).toBe(true);
    expect(directions).toEqual([-1]);
  });
});
