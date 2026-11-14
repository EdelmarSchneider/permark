// The keyboard stops of the symbols panel, pure: one tab stop for the whole list (roving tabindex) over the group
// headers and the cells, in reading order, and where each key moves it. Up | Down depend on the drawn grid (the
// nearest cell by x in the next row): the component measures those; everything else is decided here.

import type { Section } from "./model";

/** A group header or a cell (with its character); `section` is the key of the section it belongs to. */
export type Stop =
  | { key: string; kind: "header"; section: string }
  | { key: string; kind: "cell"; section: string; char: string };

/** The moves that do not need the drawn grid. */
export type Move =
  | "previous"
  | "next"
  | "groupStart"
  | "groupEnd"
  | "listStart"
  | "listEnd"
  | "previousHeader"
  | "nextHeader";

/** Between a section key and the rest of a stop key: a group name never holds U+0001. */
const SEP = "\u0001";

/** The key of a group's header. */
export function headerKey(section: string): string {
  return `${section}${SEP}h`;
}

/** The key of the cell at `index` of a section: the same character keeps its key over a redraw of the same groups. */
export function cellKey(section: string, index: number): string {
  return `${section}${SEP}${index}`;
}

/** The stops in reading order: each group's header, then its cells (the favorites have no header). */
export function stopsOf(sections: readonly Section[]): Stop[] {
  const stops: Stop[] = [];
  for (const section of sections) {
    if (section.kind === "divider") continue;
    if (section.kind === "group") {
      stops.push({ key: headerKey(section.key), kind: "header", section: section.key });
    }
    section.entries.forEach((entry, index) => {
      stops.push({
        key: cellKey(section.key, index),
        kind: "cell",
        section: section.key,
        char: entry.char,
      });
    });
  }
  return stops;
}

/** Where `move` takes the stop `current` (`null`: nowhere, it stays). */
export function moveFrom(stops: readonly Stop[], current: string, move: Move): string | null {
  const at = stops.findIndex((stop) => stop.key === current);
  if (at < 0) return stops[0]?.key ?? null;
  const here = stops[at];
  if (here === undefined) return null;
  const cells = stops.filter((stop) => stop.kind === "cell" && stop.section === here.section);
  switch (move) {
    case "previous":
      return stops[at - 1]?.key ?? null;
    case "next":
      return stops[at + 1]?.key ?? null;
    case "groupStart":
      return cells[0]?.key ?? null;
    case "groupEnd":
      return cells.at(-1)?.key ?? null;
    case "listStart":
      return stops[0]?.key ?? null;
    case "listEnd":
      return stops.at(-1)?.key ?? null;
    case "previousHeader":
      return stops.slice(0, at).findLast((stop) => stop.kind === "header")?.key ?? null;
    case "nextHeader":
      return stops.slice(at + 1).find((stop) => stop.kind === "header")?.key ?? null;
    default:
      return move satisfies never;
  }
}

/**
 * The stop that stands for `wanted` after a redraw: `wanted` while the list still has it; else a cell of the same
 * character `char` in another section (a favorite removed from the top: the character in its own group); else its
 * section's header, else its section's first cell (a group collapsed, a character gone); `null`: nothing related.
 */
export function relatedStop(
  stops: readonly Stop[],
  wanted: string | null,
  char: string | null = null,
): string | null {
  if (wanted === null) return null;
  if (stops.some((stop) => stop.key === wanted)) return wanted;
  const section = wanted.slice(0, wanted.lastIndexOf(SEP));
  if (char !== null) {
    const same = stops.find((s) => s.kind === "cell" && s.char === char && s.section !== section);
    if (same !== undefined) return same.key;
  }
  return stops.find((stop) => stop.section === section)?.key ?? null;
}

/** The stop that holds the tab stop now: the one related to `wanted`, else the first (`null`: an empty list). */
export function keptStop(
  stops: readonly Stop[],
  wanted: string | null,
  char: string | null = null,
): string | null {
  return relatedStop(stops, wanted, char) ?? stops[0]?.key ?? null;
}
