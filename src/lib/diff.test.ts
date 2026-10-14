import { describe, expect, it } from "vitest";
import { diffBlocks, oldToNew, pickAnchor } from "./diff";

describe("diffBlocks", () => {
  it("reports nothing for identical documents", () => {
    const d = diffBlocks(["a", "b", "c"], ["a", "b", "c"]);
    expect(d.added).toEqual([]);
    expect(d.removed).toEqual([]);
    expect(d.matches).toEqual([
      [0, 0],
      [1, 1],
      [2, 2],
    ]);
  });

  it("marks a changed block as added and leaves its old version as a removed run", () => {
    const d = diffBlocks(["a", "b", "c"], ["a", "B", "c"]);
    expect(d.added).toEqual([1]);
    // the marker sits after the new version, in front of the next block that survived
    expect(d.removed).toEqual([{ beforeNew: 2, oldIndices: [1] }]);
  });

  it("marks inserted blocks as added", () => {
    const d = diffBlocks(["a", "c"], ["a", "b1", "b2", "c"]);
    expect(d.added).toEqual([1, 2]);
    expect(d.removed).toEqual([]);
  });

  it("groups consecutive removed blocks into one run", () => {
    const d = diffBlocks(["a", "x", "y", "z", "b"], ["a", "b"]);
    expect(d.added).toEqual([]);
    expect(d.removed).toEqual([{ beforeNew: 1, oldIndices: [1, 2, 3] }]);
  });

  it("puts a trailing removed run at the end of the new document", () => {
    const d = diffBlocks(["a", "b", "c"], ["a"]);
    expect(d.removed).toEqual([{ beforeNew: 1, oldIndices: [1, 2] }]);
  });

  it("handles an empty old document (everything is added)", () => {
    const d = diffBlocks([], ["a", "b"]);
    expect(d.added).toEqual([0, 1]);
    expect(d.removed).toEqual([]);
  });

  it("handles an empty new document (everything is removed)", () => {
    const d = diffBlocks(["a", "b"], []);
    expect(d.added).toEqual([]);
    expect(d.removed).toEqual([{ beforeNew: 0, oldIndices: [0, 1] }]);
  });

  it("keeps duplicate blocks aligned", () => {
    const d = diffBlocks(["hr", "p", "hr"], ["hr", "p", "q", "hr"]);
    expect(d.added).toEqual([2]);
    expect(d.removed).toEqual([]);
  });

  it("finds a moved block as one removal plus one addition", () => {
    const d = diffBlocks(["a", "b", "c", "d"], ["b", "c", "d", "a"]);
    expect(d.added).toEqual([3]);
    expect(d.removed).toEqual([{ beforeNew: 0, oldIndices: [0] }]);
  });

  it("produces a valid edit script on a mixed edit", () => {
    const oldKeys = ["h1", "p1", "p2", "tbl", "code", "p3"];
    const newKeys = ["h1", "p1b", "tbl", "code2", "note", "p3"];
    const d = diffBlocks(oldKeys, newKeys);
    // matched + added covers every new block exactly once
    const covered = [...d.matches.map(([, n]) => n), ...d.added].sort((x, y) => x - y);
    expect(covered).toEqual([0, 1, 2, 3, 4, 5]);
    // matched + removed covers every old block exactly once
    const removedOld = d.removed.flatMap((r) => r.oldIndices);
    const coveredOld = [...d.matches.map(([o]) => o), ...removedOld].sort((x, y) => x - y);
    expect(coveredOld).toEqual([0, 1, 2, 3, 4, 5]);
    for (const [o, n] of d.matches) expect(oldKeys[o]).toBe(newKeys[n]);
  });

  it("falls back to replacing the middle for very large changes without throwing", () => {
    const a = Array.from({ length: 2500 }, (_, i) => `a${i}`);
    const b = Array.from({ length: 2500 }, (_, i) => `b${i}`);
    const d = diffBlocks(["head", ...a, "tail"], ["head", ...b, "tail"]);
    expect(d.matches).toEqual([
      [0, 0],
      [2501, 2501],
    ]);
    expect(d.added.length).toBe(2500);
    expect(d.removed[0].oldIndices.length).toBe(2500);
  });
});

describe("pickAnchor", () => {
  it("anchors on the first visible block that survived", () => {
    const d = diffBlocks(["a", "b", "c", "d"], ["a", "B", "c", "d"]);
    expect(pickAnchor([1, 2, 3], d)).toEqual({ oldIndex: 2, newIndex: 2 });
  });

  it("maps to the shifted index when blocks were inserted above", () => {
    const d = diffBlocks(["a", "b"], ["x", "y", "a", "b"]);
    expect(pickAnchor([0, 1], d)).toEqual({ oldIndex: 0, newIndex: 2 });
    expect(oldToNew(d).get(1)).toBe(3);
  });

  it("returns null when no visible block survived", () => {
    const d = diffBlocks(["a", "b"], ["x", "y"]);
    expect(pickAnchor([0, 1], d)).toBeNull();
  });
});
