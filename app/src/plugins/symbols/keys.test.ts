// The one tab stop of the symbols panel and where the keys move it.
import { describe, expect, it } from "vitest";
import type { CharacterEntry, CharacterGroup } from "../../api";
import { cellKey, headerKey, keptStop, moveFrom, relatedStop, stopsOf } from "./keys";
import { sectionsOf } from "./model";

function entry(char: string): CharacterEntry {
  return { char, names: [char] };
}

const GROUPS: CharacterGroup[] = [
  { name: "greek", entries: [entry("α"), entry("β")] },
  { name: "arrows", entries: [entry("→")] },
  { name: "emoji", entries: [entry("😀")] },
];

const stops = stopsOf(
  sectionsOf(GROUPS, { v: 1, chars: ["★"], groups: [] }, "", new Set(["emoji"])),
);

describe("the stops of the panel", () => {
  it("are the headers and the cells in reading order; favorites have no header, a collapsed group no cells", () => {
    expect(stops.map((s) => s.key)).toEqual([
      cellKey("\u0000favorites", 0),
      headerKey("greek"),
      cellKey("greek", 0),
      cellKey("greek", 1),
      headerKey("arrows"),
      cellKey("arrows", 0),
      headerKey("emoji"),
    ]);
  });

  it("Left | Right go through the list, across the groups", () => {
    expect(moveFrom(stops, cellKey("greek", 1), "next")).toBe(headerKey("arrows"));
    expect(moveFrom(stops, headerKey("arrows"), "previous")).toBe(cellKey("greek", 1));
    expect(moveFrom(stops, headerKey("emoji"), "next")).toBeNull();
  });

  it("Home | End stay in the group; Ctrl+Home | End span the list", () => {
    expect(moveFrom(stops, cellKey("greek", 1), "groupStart")).toBe(cellKey("greek", 0));
    expect(moveFrom(stops, headerKey("greek"), "groupEnd")).toBe(cellKey("greek", 1));
    expect(moveFrom(stops, headerKey("emoji"), "groupEnd")).toBeNull();
    expect(moveFrom(stops, cellKey("arrows", 0), "listStart")).toBe(cellKey("\u0000favorites", 0));
    expect(moveFrom(stops, cellKey("greek", 0), "listEnd")).toBe(headerKey("emoji"));
  });

  it("Page Up | Down go to the previous | next header", () => {
    expect(moveFrom(stops, cellKey("greek", 1), "previousHeader")).toBe(headerKey("greek"));
    expect(moveFrom(stops, headerKey("greek"), "previousHeader")).toBeNull();
    expect(moveFrom(stops, cellKey("greek", 0), "nextHeader")).toBe(headerKey("arrows"));
  });

  it("an unknown stop moves to the first one", () => {
    expect(moveFrom(stops, "gone", "next")).toBe(stops[0]?.key);
  });
});

describe("the stop kept over a redraw", () => {
  it("keeps the same character, else its group's header, else the first stop", () => {
    expect(keptStop(stops, cellKey("greek", 1))).toBe(cellKey("greek", 1));
    expect(keptStop(stops, cellKey("greek", 9))).toBe(headerKey("greek"));
    expect(keptStop(stops, cellKey("emoji", 0))).toBe(headerKey("emoji"));
    expect(keptStop(stops, cellKey("removed", 0))).toBe(stops[0]?.key);
    expect(keptStop(stops, null)).toBe(stops[0]?.key);
    expect(keptStop([], null)).toBeNull();
  });

  it("a favorite gone from the top: the same character in its own group, else nothing related", () => {
    const without = stopsOf(sectionsOf(GROUPS, null, "", new Set(["emoji"])));
    const gone = cellKey("\u0000favorites", 0);
    expect(relatedStop(without, gone, "β")).toBe(cellKey("greek", 1));
    expect(keptStop(without, gone, "β")).toBe(cellKey("greek", 1));
    // a character no group holds: nothing related (the panel then puts the keyboard on the favorites title)
    expect(relatedStop(without, gone, "★")).toBeNull();
    expect(keptStop(without, gone, "★")).toBe(without[0]?.key);
    // the stop itself still there wins over the character
    expect(relatedStop(stops, cellKey("greek", 0), "β")).toBe(cellKey("greek", 0));
  });
});
