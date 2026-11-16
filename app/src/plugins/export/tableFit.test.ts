// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { declarations, ruleBody } from "../../core/testing/cssRules";
import { columnWidths, fitColumns, FIT_TABLE, type ColumnSpan } from "./tableFit";

const paper = readFileSync(join(process.cwd(), "src/plugins/export/paper.css"), "utf8");

const sum = (values: number[]): number => values.reduce((total, value) => total + value, 0);

function fitted(columns: ColumnSpan[], room: number): number[] {
  const widths = columnWidths(columns, room);
  if (widths === null) throw new Error("expected widths for a table wider than its room");
  return widths;
}

describe("columnWidths", () => {
  it("leaves a table whose lines all fit to the browser", () => {
    expect(
      columnWidths(
        [
          { min: 20, max: 50 },
          { min: 30, max: 80 },
        ],
        200,
      ),
    ).toBeNull();
  });

  it("keeps short columns whole and shares the rest among long text columns by their length", () => {
    const widths = fitted(
      [
        { min: 30, max: 40 },
        { min: 40, max: 60 },
        { min: 80, max: 2000 },
        { min: 70, max: 800 },
      ],
      600,
    );
    expect(widths[0]).toBe(40);
    expect(widths[1]).toBe(60);
    expect(widths[2]).toBeCloseTo((500 * 2000) / 2800);
    expect(widths[3]).toBeCloseTo((500 * 800) / 2800);
    expect(sum(widths)).toBeCloseTo(600);
  });

  it("never puts a long column below its longest word while the words fit the room", () => {
    const widths = fitted(
      [
        { min: 150, max: 160 },
        { min: 120, max: 3000 },
        { min: 30, max: 400 },
      ],
      400,
    );
    // by length alone the first and last columns would get 18 and 45: they keep their longest words instead
    expect(widths[0]).toBeCloseTo(150);
    expect(widths[1]).toBeCloseTo(220);
    expect(widths[2]).toBeCloseTo(30);
  });

  it("keeps a short column at its natural width only when the others keep their longest words", () => {
    const widths = fitted(
      [
        { min: 0, max: 33 },
        { min: 60, max: 1000 },
        { min: 10, max: 1000 },
      ],
      100,
    );
    expect(widths[1]).toBeGreaterThanOrEqual(60);
    expect(widths[2]).toBeGreaterThanOrEqual(10);
    expect(sum(widths)).toBeCloseTo(100);
  });

  it("breaks only the words longer than an equal share when even the longest words do not fit", () => {
    const widths = fitted(
      [
        { min: 20, max: 30 },
        { min: 200, max: 400 },
        { min: 300, max: 300 },
      ],
      300,
    );
    expect(widths[0]).toBe(20);
    expect(widths[1]).toBeCloseTo(140);
    expect(widths[2]).toBeCloseTo(140);
    expect(sum(widths)).toBeCloseTo(300);
  });
});

describe("fitColumns", () => {
  it("sets the widths as shares of the table and marks the table for the fixed layout", () => {
    const table = document.createElement("table");
    table.innerHTML = "<colgroup><col></colgroup><thead><tr><th>a</th><th>b</th></tr></thead>";
    fitColumns(table, [100, 300]);
    const groups = table.querySelectorAll("colgroup");
    expect(groups).toHaveLength(1);
    expect(
      Array.from(groups[0]?.children ?? [], (col) => (col as HTMLElement).style.width),
    ).toEqual(["25%", "75%"]);
    expect(table.firstElementChild).toBe(groups[0]);
    expect(table.classList.contains(FIT_TABLE)).toBe(true);
  });
});

describe("paper rules of a fitted table", () => {
  it("lays the columns out from the widths, with no cell floor that could push it past the page", () => {
    expect(declarations(ruleBody(paper, `.md table.${FIT_TABLE}`))).toContain(
      "table-layout: fixed",
    );
    expect(declarations(ruleBody(paper, `.md .${FIT_TABLE} td`))).toContain("min-width: 0");
    expect(declarations(ruleBody(paper, `.md .${FIT_TABLE} th`))).toContain("min-width: 0");
  });
});
