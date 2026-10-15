import { describe, expect, it } from "vitest";
import { barFor, MIN_BAR_HEIGHT, type BlockBox } from "./noteBars";

const box = (
  startLine: number,
  endLine: number,
  top: number,
  bottom: number,
  depth = 0,
): BlockBox => ({ startLine, endLine, top, bottom, depth });

describe("barFor", () => {
  const page = [
    box(1, 1, 0, 40), // heading
    box(3, 5, 60, 160), // paragraph
    box(7, 12, 180, 400), // list
    box(7, 8, 180, 250, 1), // its first item (holds a nested list)
    box(8, 8, 220, 250, 2), // the nested item
    box(10, 10, 300, 330, 1), // another item
    box(14, 20, 420, 600), // table
    box(14, 14, 420, 450, 1), // header row
    box(16, 16, 480, 510, 1),
  ];

  it("spans the block that holds the passage", () => {
    expect(barFor(page, 3, 5)).toEqual({ top: 60, height: 100 });
    expect(barFor(page, 4, 4)).toEqual({ top: 60, height: 100 });
  });

  it("spans from the first block to the last when the passage runs across blocks", () => {
    expect(barFor(page, 1, 5)).toEqual({ top: 0, height: 160 });
  });

  it("takes the inner block, not the list that holds it", () => {
    expect(barFor(page, 10, 10)).toEqual({ top: 300, height: 30 });
    expect(barFor(page, 8, 8)).toEqual({ top: 220, height: 30 });
    expect(barFor(page, 16, 16)).toEqual({ top: 480, height: 30 });
  });

  it("takes the item and not the list for a passage in an item that holds a nested list", () => {
    // line 7 is the text of the first item, which the nested item (line 8) does not touch
    expect(barFor(page, 7, 7)).toEqual({ top: 180, height: 70 });
  });

  it("takes the deeper of two blocks with the same lines", () => {
    const same = [box(2, 2, 10, 90, 0), box(2, 2, 20, 50, 1)];
    expect(barFor(same, 2, 2)).toEqual({ top: 20, height: 30 });
  });

  it("returns nothing for lines no block shows, and reads a reversed range as one line", () => {
    expect(barFor(page, 30, 31)).toBeNull();
    expect(barFor([], 1, 1)).toBeNull();
    expect(barFor(page, 5, 3)).toEqual({ top: 60, height: 100 });
  });

  it("makes a bar tall enough to see", () => {
    expect(barFor([box(1, 1, 10, 12)], 1, 1)).toEqual({ top: 10, height: MIN_BAR_HEIGHT });
  });
});
