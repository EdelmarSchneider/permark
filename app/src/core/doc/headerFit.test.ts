import { describe, expect, it } from "vitest";
import { DIAGRAM_PX, editExtra, FOLD_AT, FULL_AT, headerFit, SAVE_PX } from "./headerFit";
import { PAGE_MIN } from "../shell/layout";

describe("what fits the document header", () => {
  it("wide = full, then the secondary actions fold, then the mode switch turns to icons", () => {
    expect(headerFit(1200, 0)).toBe("full");
    expect(headerFit(FULL_AT, 0)).toBe("full");
    expect(headerFit(FULL_AT - 1, 0)).toBe("fold");
    expect(headerFit(FOLD_AT, 0)).toBe("fold");
    expect(headerFit(FOLD_AT - 1, 0)).toBe("compact");
    expect(headerFit(120, 0)).toBe("compact");
    expect(headerFit(1, 0)).toBe("compact");
  });

  it("edit controls (Save, diagram toggle) fold the row sooner", () => {
    const save = editExtra(true, false);
    expect(save).toBe(SAVE_PX);
    expect(editExtra(true, true)).toBe(SAVE_PX + DIAGRAM_PX);
    expect(editExtra(false, false)).toBe(0);
    expect(headerFit(FULL_AT, save)).toBe("fold");
    expect(headerFit(FULL_AT + save, save)).toBe("full");
    expect(headerFit(FOLD_AT, save)).toBe("compact");
    expect(headerFit(FOLD_AT + save, save)).toBe("fold");
  });

  it("a width not measured yet reads as full; garbage extra counts as none", () => {
    expect(headerFit(0, 0)).toBe("full");
    expect(headerFit(-5, 0)).toBe("full");
    expect(headerFit(Number.NaN, 0)).toBe("full");
    expect(headerFit(Number.POSITIVE_INFINITY, 0)).toBe("full");
    expect(headerFit(FULL_AT, Number.NaN)).toBe("full");
    expect(headerFit(FULL_AT, -100)).toBe("full");
  });

  it("the page minimum leaves the header at least folded in View", () => {
    expect(headerFit(PAGE_MIN, 0)).toBe("fold");
  });
});
