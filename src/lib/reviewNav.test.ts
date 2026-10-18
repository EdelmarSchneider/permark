import { describe, expect, it } from "vitest";
import { indexAfterAction, recoverIndex, touchLru } from "./reviewNav";
import type { ReviewChange } from "./types";

const change = (start: number, end: number): ReviewChange => ({
  kind: "modified",
  old: { startByte: start, endByte: end },
  new: { startByte: start, endByte: end },
  oldBlocks: { start: 0, end: 1 },
  newBlocks: { start: 0, end: 1 },
  newLine: 1,
  rows: [],
});

describe("indexAfterAction", () => {
  it("lands on the change that slides into the one just acted on", () => {
    expect(indexAfterAction(3, 0)).toBe(0);
    expect(indexAfterAction(3, 1)).toBe(1);
  });

  it("lands on the previous change when the last one was acted on", () => {
    expect(indexAfterAction(3, 2)).toBe(1);
    expect(indexAfterAction(1, 0)).toBe(0);
  });
});

describe("recoverIndex", () => {
  const changes = [change(0, 10), change(20, 30), change(40, 50)];

  it("stays on the change whose old range still holds the place", () => {
    expect(recoverIndex(changes, 25)).toBe(1);
  });

  it("takes the nearest change after the place when the old one is gone", () => {
    expect(recoverIndex(changes, 15)).toBe(1);
    expect(recoverIndex(changes, 60)).toBe(2);
  });
});

describe("touchLru", () => {
  it("drops the oldest path once the list is full", () => {
    const { kept, dropped } = touchLru(["a", "b", "c", "d", "e"], "f", 5);
    expect(kept).toEqual(["f", "a", "b", "c", "d"]);
    expect(dropped).toEqual(["e"]);
  });

  it("moves a path that is visited again to the front and drops nothing", () => {
    const { kept, dropped } = touchLru(["a", "b", "c"], "b", 5);
    expect(kept).toEqual(["b", "a", "c"]);
    expect(dropped).toEqual([]);
  });
});
