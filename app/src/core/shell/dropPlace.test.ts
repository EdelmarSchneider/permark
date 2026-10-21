import { describe, expect, it } from "vitest";
import { badgePlace, popoverPlace } from "./dropPlace";

const view = { width: 1000, height: 700 };

describe("the badge", () => {
  const badge = { width: 200, height: 24 };

  it("sits above-right of the pointer", () => {
    expect(badgePlace({ x: 100, y: 300 }, badge, view)).toEqual({ x: 114, y: 270 });
  });

  it("flips left near the right edge and down near the top", () => {
    expect(badgePlace({ x: 900, y: 300 }, badge, view)).toEqual({ x: 686, y: 270 });
    expect(badgePlace({ x: 100, y: 10 }, badge, view)).toEqual({ x: 114, y: 28 });
  });

  it("stays 8 px inside the view, whatever the point", () => {
    for (const at of [
      { x: -50, y: -50 },
      { x: 5000, y: 5000 },
      { x: 0, y: 699 },
    ]) {
      const p = badgePlace(at, badge, view);
      expect(p.x).toBeGreaterThanOrEqual(8);
      expect(p.y).toBeGreaterThanOrEqual(8);
      expect(p.x + badge.width).toBeLessThanOrEqual(view.width - 8);
      expect(p.y + badge.height).toBeLessThanOrEqual(view.height - 8);
    }
  });
});

describe("the question", () => {
  const box = { width: 340, height: 200 };

  it("top-left 8 px off the drop point", () => {
    expect(popoverPlace({ x: 100, y: 100 }, box, view)).toEqual({ x: 108, y: 108 });
  });

  it("flips to stay inside near the right and bottom edges", () => {
    expect(popoverPlace({ x: 900, y: 600 }, box, view)).toEqual({ x: 552, y: 392 });
  });

  it("a narrow view centers it across, at the point's height", () => {
    const narrow = { width: 340, height: 700 };
    const p = popoverPlace({ x: 300, y: 100 }, { width: 324, height: 200 }, narrow);
    expect(p).toEqual({ x: 8, y: 108 });
  });

  it("a box larger than the view starts at the margin", () => {
    expect(popoverPlace({ x: 50, y: 50 }, { width: 2000, height: 2000 }, view)).toEqual({
      x: 8,
      y: 8,
    });
  });
});
