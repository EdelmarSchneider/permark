import { describe, expect, it } from "vitest";
import { linesOf } from "../../core/doc/largeText";
import {
  columns,
  INDENT,
  lineAtRow,
  MIN_COLS,
  piecesFor,
  rowOffset,
  rowsOf,
  rowStarts,
  rowsText,
  widthsOf,
  anchorAt,
  intoLine,
  leadFor,
  measure,
  MAX_LEAD,
} from "./wrapRows";

describe("the columns of a line", () => {
  it("counts plain text by its characters", () => {
    expect(columns("")).toBe(0);
    expect(columns("hello world")).toBe(11);
  });

  it("moves a tab to the next stop of 8, drops a carriage return", () => {
    expect(columns("\tx")).toBe(9);
    expect(columns("abc\tx")).toBe(9);
    expect(columns("abcdefgh\tx")).toBe(17);
    expect(columns("ab\r")).toBe(2);
  });

  it("counts wide characters twice, combining marks not at all, a pair once", () => {
    expect(columns("日本語")).toBe(6);
    expect(columns("가나")).toBe(4);
    expect(columns("é")).toBe(1);
    expect(columns("😀a")).toBe(3);
    expect(columns("𝒜")).toBe(1);
  });
});

describe("the rows of a line", () => {
  it("is one row up to the width, then rows of the width less the indent", () => {
    expect(rowsOf(0, 80)).toBe(1);
    expect(rowsOf(80, 80)).toBe(1);
    expect(rowsOf(81, 80)).toBe(2);
    expect(rowsOf(80 + 78, 80)).toBe(2);
    expect(rowsOf(80 + 78 + 1, 80)).toBe(3);
  });

  it("never counts with fewer columns than the least (a very narrow pane)", () => {
    expect(rowsOf(MIN_COLS, 1)).toBe(1);
    expect(rowsOf(MIN_COLS + 1, 0)).toBe(2);
    expect(rowsOf(1000, -5)).toBe(rowsOf(1000, MIN_COLS));
  });

  it("starts each row where the one before it ends", () => {
    expect(rowOffset(0, 80)).toBe(0);
    expect(rowOffset(1, 80)).toBe(80);
    expect(rowOffset(2, 80)).toBe(80 + 80 - INDENT);
    const line = "x".repeat(500);
    const rows = rowsOf(500, 80);
    const joined = Array.from({ length: rows }, (_, r) => rowsText(line, r, r + 1, 80)).join("");
    expect(joined).toBe(line);
  });

  it("never cuts a surrogate pair between two pieces", () => {
    const line = `${"a".repeat(79)}😀${"b".repeat(100)}`;
    const head = rowsText(line, 0, 1, 80);
    const tail = rowsText(line, 1, 3, 80);
    expect(head + tail).toBe(line);
    expect(head.endsWith("a")).toBe(true);
    expect(tail.startsWith("😀")).toBe(true);
  });
});

describe("where the lines start, in rows", () => {
  it("sums the rows above each line and finds the line of a row", () => {
    const starts = rowStarts([10, 200, 0, 81], 80);
    expect([...starts]).toEqual([0, 1, 4, 5, 7]);
    expect(lineAtRow(starts, 0)).toEqual({ line: 0, row: 0 });
    expect(lineAtRow(starts, 1)).toEqual({ line: 1, row: 0 });
    expect(lineAtRow(starts, 3)).toEqual({ line: 1, row: 2 });
    expect(lineAtRow(starts, 4)).toEqual({ line: 2, row: 0 });
    expect(lineAtRow(starts, 6)).toEqual({ line: 3, row: 1 });
    // past the end: the last row; before the start: the first
    expect(lineAtRow(starts, 99)).toEqual({ line: 3, row: 1 });
    expect(lineAtRow(starts, -4)).toEqual({ line: 0, row: 0 });
    expect(lineAtRow(rowStarts([], 80), 3)).toEqual({ line: 0, row: 0 });
  });

  it("draws short lines whole and a very long one only by the rows asked for", () => {
    // a short line, a line of 1 000 rows, a short line
    const starts = rowStarts([5, 80 + 999 * 78, 5], 80);
    expect(starts[2]).toBe(1001);
    // from the middle of the long line: only its rows from there
    expect(piecesFor(starts, 500, 30, 200)).toEqual({
      top: 500,
      pieces: [{ line: 1, from: 499, to: 529 }],
    });
    // across its end into the next line
    expect(piecesFor(starts, 995, 30, 200)).toEqual({
      top: 995,
      pieces: [
        { line: 1, from: 994, to: 1000 },
        { line: 2, from: 0, to: 1 },
      ],
    });
    // a line of few rows is drawn from its first row even when only its end is asked for
    const short = rowStarts([80 + 78 * 4, 3], 80);
    expect(piecesFor(short, 3, 10, 200)).toEqual({
      top: 0,
      pieces: [
        { line: 0, from: 0, to: 5 },
        { line: 1, from: 0, to: 1 },
      ],
    });
    expect(piecesFor(starts, 0, 0, 200)).toEqual({ top: 0, pieces: [] });
  });

  it("keeps the widths of the lines a chunk did not change", () => {
    const before = ["one", "two", "thr"];
    const kept = { lines: before, ...widthsOf(before, null) };
    // the next chunk went on with the last line
    const after = ["one", "two", "three and more", "four"];
    const next = Object.assign([...after], { 0: before[0], 1: before[1] });
    expect([...widthsOf(next, kept).widths]).toEqual([3, 3, 14, 4]);
    // another text: all counted again
    expect([...widthsOf(["xxxxxxx", "y"], kept).widths]).toEqual([7, 1]);
  });
});

describe("very long texts (adversarial)", () => {
  it("counts 10 MB of 50 000-character lines at once and draws only a screenful", () => {
    const line = "abcdefghij".repeat(5000);
    const text = `${Array.from({ length: 200 }, () => line).join("\n")}\n`;
    const startedAt = performance.now();
    const { lines } = linesOf(text);
    const starts = rowStarts(widthsOf(lines, null).widths, 120);
    const drawn = piecesFor(starts, 12_345, 140, 200);
    const took = performance.now() - startedAt;
    expect(lines).toHaveLength(200);
    expect(starts[200]).toBe(200 * rowsOf(50_000, 120));
    const chars = drawn.pieces.reduce(
      (sum, p) => sum + rowsText(lines[p.line] ?? "", p.from, p.to, 120).length,
      0,
    );
    expect(chars).toBeLessThanOrEqual(141 * 120);
    expect(took).toBeLessThan(2000);
  });

  it("draws a few rows of a single line of 5 MB", () => {
    const line = "0123456789".repeat(500_000);
    const starts = rowStarts(widthsOf([line], null).widths, 100);
    const total = starts[1] ?? 0;
    expect(total).toBe(rowsOf(5_000_000, 100));
    const end = piecesFor(starts, total - 10, 140, 200);
    expect(end.pieces).toHaveLength(1);
    const piece = end.pieces[0];
    expect(piece?.to).toBe(total);
    const shown = rowsText(line, piece?.from ?? 0, piece?.to ?? 0, 100);
    expect(shown.length).toBeLessThanOrEqual(10 * 98);
    expect(line.endsWith(shown)).toBe(true);
  });

  it("counts a line of 5 MB with wide characters without stalling", () => {
    const line = "日本語テキスト ".repeat(600_000);
    const startedAt = performance.now();
    const width = columns(line);
    expect(width).toBe(600_000 * 15);
    expect(performance.now() - startedAt).toBeLessThan(2000);
  });
});

describe("indented lines (their further rows keep the indentation)", () => {
  it("measures the leading spaces and tabs of a line", () => {
    expect(measure("    x = 1")).toEqual({ width: 9, lead: 4 });
    expect(measure("\tx")).toEqual({ width: 9, lead: 8 });
    expect(measure("x")).toEqual({ width: 1, lead: 0 });
    expect(measure("   ")).toEqual({ width: 3, lead: 3 });
    expect(measure("  日本")).toEqual({ width: 6, lead: 2 });
  });

  it("counts further rows narrower by the indentation, at most half a row and MAX_LEAD", () => {
    expect(leadFor(4, 80)).toBe(4);
    expect(leadFor(60, 80)).toBe(MAX_LEAD);
    expect(leadFor(20, 30)).toBe(14);
    // 80 + 74 + 74 columns: three rows with an indentation of 4
    expect(rowsOf(80 + 74 * 2, 80, 4)).toBe(3);
    expect(rowsOf(80 + 74 * 2 + 1, 80, 4)).toBe(4);
    expect(rowOffset(2, 80, 4)).toBe(80 + 74);
    const line = `    ${"y".repeat(600)}`;
    const rows = rowsOf(line.length, 80, 4);
    const joined = Array.from({ length: rows }, (_, r) => rowsText(line, r, r + 1, 80, 4)).join("");
    expect(joined).toBe(line);
  });

  it("keeps the indentation of the lines a chunk did not change", () => {
    const before = ["  one", "two"];
    const kept = { lines: before, ...widthsOf(before, null) };
    expect([...widthsOf(["  one", "two more", "   x"], kept).leads]).toEqual([2, 0, 3]);
  });
});

describe("the line kept at the top when wrapping switches (adversarial)", () => {
  const row = 21.6;

  it("never takes the line above when the pane's edge is a hair above a line's top (rounding)", () => {
    // line 61 at the top: its top is 60 rows down; the pane's edge reads a fraction of a pixel less
    expect(anchorAt(60 * row - 0.01, row, 140)).toEqual({ line: 61, delta: 0 });
    expect(anchorAt(60 * row - 0.6, row, 140).line).toBe(61);
    expect(anchorAt(60 * row + 5, row, 140)).toEqual({ line: 61, delta: expect.closeTo(5, 5) });
    // a line more than a pixel in view is the one at the top
    expect(anchorAt(60 * row - 2, row, 140).line).toBe(60);
    expect(anchorAt(-30, row, 140)).toEqual({ line: 1, delta: 0 });
    expect(anchorAt(1e9, row, 140).line).toBe(140);
  });

  it("puts the line back at the top whatever the long line above it grew to (7 665 and 520 characters)", () => {
    for (const above of [7665, 520]) {
      // the log: line 60 is the long one, every other line short; line 61 was at the top, not wrapping
      const widths = Array.from({ length: 140 }, (_, i) => (i === 59 ? above : 40));
      const at = anchorAt(60 * row - 0.01, row, widths.length);
      expect(at.line).toBe(61);
      // wrapping on: the line's top is the rows above it; the pane goes there, within its first row
      const starts = rowStarts(widths, 120);
      const top = (starts[at.line - 1] ?? 0) * row + intoLine(at.delta, row);
      expect(top).toBeCloseTo((59 + rowsOf(above, 120)) * row, 5);
      expect(rowsOf(above, 120)).toBeGreaterThan(1);
    }
  });

  it("starts again within the first row of the line, never deeper into a line that grew", () => {
    expect(intoLine(15, row)).toBe(15);
    expect(intoLine(300, row)).toBeCloseTo(row - 1, 5);
    expect(intoLine(-3, row)).toBe(0);
  });
});
