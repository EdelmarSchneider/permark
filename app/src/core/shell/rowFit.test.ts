import { describe, expect, it } from "vitest";
import { rowFit } from "./rowFit";

describe("rowFit", () => {
  // 228 px of room, 110 px that stay whole, five 26 px buttons, a 26 px menu button once one of them moves out
  it("keeps every button while they all fit, with no menu button", () => {
    expect(rowFit({ room: 240, fixed: 110, count: 5, step: 26, more: 26 })).toBe(5);
    expect(rowFit({ room: 300, fixed: 110, count: 5, step: 26, more: 26 })).toBe(5);
  });

  it("moves buttons out until the rest and the menu button fit", () => {
    // one moved out would not help (the menu button takes its room): two go
    expect(rowFit({ room: 228, fixed: 110, count: 5, step: 26, more: 26 })).toBe(3);
    expect(rowFit({ room: 239, fixed: 110, count: 5, step: 26, more: 26 })).toBe(3);
    // a smaller menu button leaves room for one more
    expect(rowFit({ room: 239, fixed: 110, count: 5, step: 26, more: 20 })).toBe(4);
  });

  it("moves them all out when the part that stays whole leaves no room", () => {
    expect(rowFit({ room: 120, fixed: 110, count: 5, step: 26, more: 26 })).toBe(0);
    expect(rowFit({ room: 50, fixed: 110, count: 5, step: 26, more: 26 })).toBe(0);
  });

  it("a row not measured yet (no room, nothing to keep) keeps every button: the first frame never folds", () => {
    expect(rowFit({ room: 0, fixed: 110, count: 5, step: 26, more: 26 })).toBe(5);
    expect(rowFit({ room: 228, fixed: 0, count: 5, step: 26, more: 26 })).toBe(5);
    expect(rowFit({ room: Number.NaN, fixed: 110, count: 5, step: 26, more: 26 })).toBe(5);
  });

  it("nothing to move out: zero", () => {
    expect(rowFit({ room: 100, fixed: 110, count: 0, step: 26, more: 26 })).toBe(0);
  });
});
