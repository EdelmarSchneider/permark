import { describe, expect, it } from "vitest";
import { changedLines } from "./textDiff";

describe("the lines that changed between two versions of a text", () => {
  it("says nothing for the same text", () => {
    expect(changedLines("a\nb\n", "a\nb\n")).toEqual({ added: [], removedAfter: [] });
  });

  it("names added and replaced lines by their place in the new text", () => {
    expect(changedLines("a\nb\nc\n", "a\nB\nc\nd\n")).toEqual({
      added: [
        { first: 2, last: 2 },
        { first: 4, last: 4 },
      ],
      removedAfter: [],
    });
    // a run of new lines is one range
    expect(changedLines("a\nz\n", "a\nx\ny\nz\n").added).toEqual([{ first: 2, last: 3 }]);
  });

  it("marks where lines went, after which line of the new text", () => {
    expect(changedLines("a\nb\nc\n", "a\nc\n")).toEqual({ added: [], removedAfter: [1] });
    // the first line went: before the first
    expect(changedLines("a\nb\n", "b\n")).toEqual({ added: [], removedAfter: [0] });
  });

  it("treats a replaced line as new (and nothing is marked as gone)", () => {
    expect(changedLines("a\nb\nc\n", "a\nX\nc\n")).toEqual({
      added: [{ first: 2, last: 2 }],
      removedAfter: [],
    });
  });

  it("reads CRLF text the same as the text it was before", () => {
    expect(changedLines("a\r\nb\r\n", "a\r\nb\r\nc\r\n").added).toEqual([{ first: 3, last: 3 }]);
    // a text that gained a final line break changes nothing the reader sees
    expect(changedLines("a\nb", "a\nb\n").added).toEqual([]);
  });

  it("keeps the number of ranges bounded for a text that changed everywhere", () => {
    const before = Array.from({ length: 2000 }, (_, i) => `same ${i}`).join("\n");
    const after = before
      .split("\n")
      .map((line, i) => (i % 2 === 0 ? `changed ${i}` : line))
      .join("\n");
    const change = changedLines(before, after);
    expect(change.added.length).toBeLessThanOrEqual(200);
    expect(change.added.length).toBeGreaterThan(0);
  });
});
