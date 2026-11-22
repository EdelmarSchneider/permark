import { describe, expect, it } from "vitest";
import { changedSections, currentIndex, depths } from "./outline";

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
  const at = (tops: readonly number[], limit: number): number =>
    currentIndex(tops.length, (i) => tops[i] ?? Infinity, limit);

  it("is the last heading at or above the line", () => {
    const tops = [-500, -200, 10, 400, 900];
    expect(at(tops, 0)).toBe(1);
    expect(at(tops, 10)).toBe(2);
    expect(at(tops, 399)).toBe(2);
    expect(at(tops, 10_000)).toBe(4);
  });

  it("is -1 before the first heading and for no headings", () => {
    expect(at([50, 100], 0)).toBe(-1);
    expect(at([], 0)).toBe(-1);
  });

  it("finds what a scan from the first top finds, reading a handful of tops out of 10 000", () => {
    // 10 000 places 37 px apart, some at the same top (an empty block), the ones not built yet far below
    const tops = Array.from({ length: 10_000 }, (_, i) =>
      i >= 9_990 ? Infinity : Math.floor(i / 3) * 37 - 50_000,
    );
    const linear = (limit: number): number => {
      let found = -1;
      tops.forEach((top, i) => {
        if (top <= limit) found = i;
      });
      return found;
    };
    for (const limit of [-60_000, -50_000, -49_999, 0, 40, 12_345, 73_000, Infinity]) {
      let reads = 0;
      const found = currentIndex(
        tops.length,
        (i) => {
          reads += 1;
          return tops[i] ?? Infinity;
        },
        limit,
      );
      expect(found, `limit ${limit}`).toBe(linear(limit));
      expect(reads, `limit ${limit}`).toBeLessThanOrEqual(20);
    }
  });
});

describe("changedSections", () => {
  const blocks = ["intro", "h1", "p1", "h2", "p2", "p3", "h3"].map((id) => ({ id }));
  const items = [{ block: "h1" }, { block: "h2" }, { block: "h3" }];

  it("puts a block in the section of the last heading at or before it", () => {
    expect([...changedSections(items, blocks, ["p1"])]).toEqual([0]);
    expect([...changedSections(items, blocks, ["p3", "p2"])]).toEqual([1]);
    expect([...changedSections(items, blocks, ["h3"])].sort()).toEqual([2]);
    expect([...changedSections(items, blocks, ["p1", "h3"])].sort()).toEqual([0, 2]);
  });

  it("marks nothing for blocks before the first heading, unknown ids or no headings", () => {
    expect(changedSections(items, blocks, ["intro"]).size).toBe(0);
    expect(changedSections(items, blocks, ["gone"]).size).toBe(0);
    expect(changedSections([], blocks, ["p1"]).size).toBe(0);
    expect(changedSections(items, [], ["p1"]).size).toBe(0);
  });

  it("skips a heading whose block is not in the list", () => {
    const odd = [{ block: "h1" }, { block: "missing" }, { block: "h3" }];
    expect([...changedSections(odd, blocks, ["p2"])]).toEqual([0]);
  });
});
