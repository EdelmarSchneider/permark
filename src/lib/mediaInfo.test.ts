import { describe, expect, it } from "vitest";
import { formatDimensions, formatDuration, formatSize, mediaSummary } from "./mediaInfo";

describe("formatSize", () => {
  it.each([
    [0, "0 B"],
    [512, "512 B"],
    [1024, "1 KB"],
    [312 * 1024, "312 KB"],
    [1536 * 1024, "1.5 MB"],
    [5 * 1024 * 1024, "5.0 MB"],
    [250 * 1024 * 1024, "250 MB"],
    [3 * 1024 ** 3, "3.0 GB"],
  ])("%d bytes is %s", (bytes, text) => {
    expect(formatSize(bytes)).toBe(text);
  });

  it("does not go below zero or past the largest unit", () => {
    expect(formatSize(-5)).toBe("0 B");
    expect(formatSize(5 * 1024 ** 5)).toBe("5,120 TB");
  });
});

describe("formatDuration", () => {
  it.each([
    [0, "0:00"],
    [7, "0:07"],
    [59.6, "1:00"],
    [192, "3:12"],
    [3723, "1:02:03"],
  ])("%d seconds is %s", (seconds, text) => {
    expect(formatDuration(seconds)).toBe(text);
  });

  it("says nothing for a length that is not known", () => {
    expect(formatDuration(Number.NaN)).toBe("");
    expect(formatDuration(Number.POSITIVE_INFINITY)).toBe("");
    expect(formatDuration(-1)).toBe("");
  });
});

describe("formatDimensions", () => {
  it("writes the pixel size once it is known", () => {
    expect(formatDimensions({ width: 800, height: 450 })).toBe("800 × 450 px");
    expect(formatDimensions({})).toBe("");
    expect(formatDimensions({ width: 0, height: 0 })).toBe("");
  });
});

describe("mediaSummary", () => {
  it("gives a picture its pixels and its size", () => {
    expect(mediaSummary("image", 312 * 1024, { width: 800, height: 450 })).toEqual([
      "800 × 450 px",
      "312 KB",
    ]);
    // before the picture is decoded only the size is known
    expect(mediaSummary("image", 2048, {})).toEqual(["2 KB"]);
  });

  it("gives a sound its length and its size", () => {
    expect(mediaSummary("audio", 4 * 1024 * 1024, { duration: 192 })).toEqual(["3:12", "4.0 MB"]);
    expect(mediaSummary("audio", 4 * 1024 * 1024, {})).toEqual(["4.0 MB"]);
  });

  it("gives a video its pixels, its length and its size", () => {
    expect(
      mediaSummary("video", 12 * 1024 * 1024, { width: 1280, height: 720, duration: 65 }),
    ).toEqual(["1280 × 720 px", "1:05", "12.0 MB"]);
  });
});
