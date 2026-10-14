import { describe, expect, it } from "vitest";
import { viewBoxWidth } from "./breakout";

describe("viewBoxWidth", () => {
  it("reads the width of a view box", () => {
    expect(viewBoxWidth("0 0 812 300")).toBe(812);
    expect(viewBoxWidth("-8 -8 1204.5 96")).toBe(1205);
    expect(viewBoxWidth("0,0,640,480")).toBe(640);
    expect(viewBoxWidth("  0   0 99 10 ")).toBe(99);
  });

  it("is null for anything that is not a usable view box", () => {
    for (const value of [
      null,
      undefined,
      "",
      "0 0 0 10",
      "0 0 -5 10",
      "0 0 abc 10",
      "0 0 10",
      "1 2 3 4 5",
    ]) {
      expect(viewBoxWidth(value)).toBeNull();
    }
  });
});
