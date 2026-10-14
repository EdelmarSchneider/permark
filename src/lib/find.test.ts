import { describe, expect, it } from "vitest";
import { clampIndex, findMatches, foldQuery, stepIndex, type FindSegment } from "./find";

const seg = (text: string, blockStart = false): FindSegment => ({ text, blockStart });

describe("findMatches", () => {
  it("finds every occurrence, case and accent insensitive", () => {
    const m = findMatches([seg("Watcher, watcher and WÁTCHER", true)], "watcher");
    expect(m).toHaveLength(3);
    expect(m[0]).toEqual({ startSeg: 0, startOff: 0, endSeg: 0, endOff: 7 });
    expect(m[2]).toEqual({ startSeg: 0, startOff: 21, endSeg: 0, endOff: 28 });
  });

  it("folds accents in the query too", () => {
    expect(findMatches([seg("a saúde do sistema")], "SAUDE")).toHaveLength(1);
    expect(findMatches([seg("saude")], "saúde")).toHaveLength(1);
  });

  it("matches across text nodes inside one block", () => {
    const segments = [seg("never produce ", true), seg("more than one"), seg(" toast")];
    const [m] = findMatches(segments, "produce more than one toast");
    expect(m).toEqual({ startSeg: 0, startOff: 6, endSeg: 2, endOff: 6 });
  });

  it("does not match across blocks", () => {
    const segments = [seg("first paragraph end", true), seg("start of second", true)];
    expect(findMatches(segments, "end start")).toHaveLength(0);
    expect(findMatches(segments, "endstart")).toHaveLength(0);
  });

  it("collapses whitespace runs and newlines", () => {
    expect(findMatches([seg("foo\n   bar")], "foo bar")).toHaveLength(1);
    expect(findMatches([seg("foo bar")], "foo   bar")).toHaveLength(1);
  });

  it("returns nothing for an empty or whitespace query", () => {
    expect(findMatches([seg("abc")], "")).toEqual([]);
    expect(findMatches([seg("abc")], "   ")).toEqual([]);
  });

  it("handles astral characters when mapping offsets", () => {
    const [m] = findMatches([seg("😀 note")], "note");
    expect(m.startOff).toBe(3);
    expect(m.endOff).toBe(7);
  });

  it("terms mode matches any word and merges overlaps", () => {
    const segments = [seg("Character notes: tall. The descriptions change.", true)];
    const phrase = findMatches(segments, "character descriptions", "phrase");
    expect(phrase).toHaveLength(0);
    const terms = findMatches(segments, "character descriptions", "terms");
    expect(terms).toHaveLength(2);
    expect(terms[0].startOff).toBe(0);
  });

  it("caps the number of matches", () => {
    const text = "a ".repeat(100);
    expect(findMatches([seg(text)], "a", "phrase", 10)).toHaveLength(10);
  });
});

describe("foldQuery", () => {
  it("normalizes case, accents and whitespace", () => {
    expect(foldQuery("  Café   Crème ")).toBe("cafe creme");
  });
});

describe("stepIndex", () => {
  it("wraps around in both directions", () => {
    expect(stepIndex(2, 3, 1)).toBe(0);
    expect(stepIndex(0, 3, -1)).toBe(2);
    expect(stepIndex(1, 3, 1)).toBe(2);
  });
  it("starts at the ends when nothing is selected, and ignores empty results", () => {
    expect(stepIndex(-1, 3, 1)).toBe(0);
    expect(stepIndex(-1, 3, -1)).toBe(2);
    expect(stepIndex(0, 0, 1)).toBe(-1);
  });
});

describe("clampIndex", () => {
  it("keeps the current match after a reload when possible", () => {
    expect(clampIndex(4, 12)).toBe(4);
    expect(clampIndex(9, 5)).toBe(4);
    expect(clampIndex(3, 0)).toBe(-1);
    expect(clampIndex(-1, 5)).toBe(0);
  });
});
