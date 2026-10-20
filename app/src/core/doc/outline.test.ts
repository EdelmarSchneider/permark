import { describe, expect, it } from "vitest";
import { currentIndex, depths } from "./outline";

const levels = (...l: number[]) => l.map((level) => ({ level }));

describe("depths", () => {
  it("indents by structure, not by level number", () => {
    expect(depths(levels(1, 2, 3, 2, 1, 2))).toEqual([0, 1, 2, 1, 0, 1]);
    expect(depths(levels(2, 4, 6, 4))).toEqual([0, 1, 2, 1]);
    expect(depths(levels(3, 1, 2))).toEqual([0, 0, 1]);
    expect(depths(levels(1, 1, 1))).toEqual([0, 0, 0]);
    expect(depths([])).toEqual([]);
  });
});

describe("currentIndex", () => {
  it("is the last heading at or above the line", () => {
    const tops = [-500, -200, 10, 400, 900];
    expect(currentIndex(tops, 0)).toBe(1);
    expect(currentIndex(tops, 10)).toBe(2);
    expect(currentIndex(tops, 399)).toBe(2);
    expect(currentIndex(tops, 10_000)).toBe(4);
  });

  it("is -1 before the first heading and for no headings", () => {
    expect(currentIndex([50, 100], 0)).toBe(-1);
    expect(currentIndex([], 0)).toBe(-1);
  });
});
