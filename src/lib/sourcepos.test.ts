import { describe, expect, it } from "vitest";
import { blockForLine, parseSourcepos } from "./sourcepos";

describe("parseSourcepos", () => {
  it("parses comrak positions", () => {
    expect(parseSourcepos("12:1-15:3")).toEqual({ startLine: 12, endLine: 15 });
  });
  it("rejects missing or malformed values", () => {
    expect(parseSourcepos(null)).toBeNull();
    expect(parseSourcepos("")).toBeNull();
    expect(parseSourcepos("12-15")).toBeNull();
  });
});

describe("blockForLine", () => {
  const ranges = [
    { startLine: 1, endLine: 1 },
    { startLine: 3, endLine: 6 },
    null,
    { startLine: 8, endLine: 8 },
  ];
  it("finds the block that covers the line", () => {
    expect(blockForLine(ranges, 1)).toBe(0);
    expect(blockForLine(ranges, 5)).toBe(1);
    expect(blockForLine(ranges, 8)).toBe(3);
  });
  it("uses the closest earlier block for a line in a gap", () => {
    expect(blockForLine(ranges, 2)).toBe(0);
    expect(blockForLine(ranges, 7)).toBe(1);
  });
  it("returns the last block for lines past the end and null without position data", () => {
    expect(blockForLine(ranges, 99)).toBe(3);
    expect(blockForLine([null, null], 4)).toBeNull();
  });
});
