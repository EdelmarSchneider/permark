import { describe, expect, it } from "vitest";
import {
  DEFAULT_WIDTH,
  READING_WIDTHS,
  isReadingWidth,
  nextWidth,
  normalizeWidth,
} from "./readingWidth";

describe("normalizeWidth", () => {
  it("keeps every known width", () => {
    for (const width of READING_WIDTHS) expect(normalizeWidth(width)).toBe(width);
  });

  it("migrates the old wide flag when no width was saved", () => {
    expect(normalizeWidth(undefined, true)).toBe("wide");
    expect(normalizeWidth(undefined, false)).toBe("comfortable");
    expect(normalizeWidth(null, true)).toBe("wide");
    expect(normalizeWidth(undefined, undefined)).toBe(DEFAULT_WIDTH);
  });

  it("lets a saved width win over the old flag", () => {
    expect(normalizeWidth("full", true)).toBe("full");
    expect(normalizeWidth("comfortable", true)).toBe("comfortable");
  });

  it("falls back to the default for values it does not know", () => {
    for (const value of ["ultrawide", "", 3, true, {}, []]) {
      expect(normalizeWidth(value)).toBe(DEFAULT_WIDTH);
    }
    // an unknown width counts as not saved, so the old flag (if any) still applies
    expect(normalizeWidth("ultrawide", true)).toBe("wide");
    // only a real `true` counts as the old wide flag
    expect(normalizeWidth(undefined, "yes")).toBe(DEFAULT_WIDTH);
    expect(normalizeWidth(undefined, 1)).toBe(DEFAULT_WIDTH);
  });
});

describe("nextWidth", () => {
  it("cycles Comfortable, Wide, Full and back", () => {
    expect(nextWidth("comfortable")).toBe("wide");
    expect(nextWidth("wide")).toBe("full");
    expect(nextWidth("full")).toBe("comfortable");
  });

  it("returns to the start after one lap", () => {
    let width = DEFAULT_WIDTH;
    for (let i = 0; i < READING_WIDTHS.length; i++) width = nextWidth(width);
    expect(width).toBe(DEFAULT_WIDTH);
  });
});

describe("isReadingWidth", () => {
  it("accepts only the three names", () => {
    expect(isReadingWidth("wide")).toBe(true);
    expect(isReadingWidth("Wide")).toBe(false);
    expect(isReadingWidth(undefined)).toBe(false);
  });
});
