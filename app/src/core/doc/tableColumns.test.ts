// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { codeOnly, declarations, ruleBody } from "../testing/cssRules";
import {
  columnWidths,
  fitColumns,
  FIT_TABLE,
  measureColumns,
  NATURAL_TABLE,
  SCREEN_TABLE,
  screenLayout,
  type ColumnSpan,
  type TableLayout,
} from "./tableColumns";

const paper = readFileSync(join(process.cwd(), "src/plugins/export/paper.css"), "utf8");
const screen = readFileSync(join(process.cwd(), "src/core/doc/markdown.css"), "utf8");

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
    fitColumns(table, [100, 300], FIT_TABLE);
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

/** Short cells end at 100px, long text is capped at 330px (border box, as measured). */
const LIMITS = { short: 100, cap: 330 };

function widthsOf(layout: TableLayout): number[] {
  if (layout.kind !== "fill" && layout.kind !== "scroll")
    throw new Error(`no widths: ${layout.kind}`);
  return layout.widths;
}

describe("screenLayout", () => {
  it("keeps a table whose lines fit at its own width when it is well short of the column", () => {
    const columns = [
      { min: 60, max: 80 },
      { min: 70, max: 150 },
    ];
    expect(screenLayout(columns, 800, LIMITS)).toEqual({ kind: "natural" });
    // most of the column: it fills the column (the browser shares the rest), every line whole
    expect(screenLayout(columns, 300, LIMITS)).toEqual({ kind: "auto" });
  });

  it("keeps id, date and number columns on one line and gives the rest to the long text", () => {
    const widths = widthsOf(
      screenLayout(
        [
          { min: 60, max: 70 },
          { min: 80, max: 98 },
          { min: 70, max: 90 },
          { min: 90, max: 2400 },
        ],
        700,
        LIMITS,
      ),
    );
    expect(widths.slice(0, 3)).toEqual([70, 98, 90]);
    expect(widths[3]).toBeCloseTo(442);
    expect(sum(widths)).toBeCloseTo(700);
  });

  it("keeps a short column whole even where an equal share would wrap it", () => {
    // four columns in 300px: an equal share is 75px, the 96px date still stays on one line
    const widths = widthsOf(
      screenLayout(
        [
          { min: 70, max: 96 },
          { min: 60, max: 800 },
          { min: 60, max: 700 },
          { min: 60, max: 400 },
        ],
        300,
        LIMITS,
      ),
    );
    expect(widths[0]).toBe(96);
    for (const width of widths.slice(1)) expect(width).toBeGreaterThanOrEqual(60);
    expect(sum(widths)).toBeCloseTo(300);
  });

  it("scrolls only when the floors do not fit: long text is capped, short cells stay whole", () => {
    const layout = screenLayout(
      [
        { min: 60, max: 90 },
        { min: 120, max: 3000 },
        { min: 150, max: 200 },
        { min: 110, max: 2000 },
      ],
      400,
      LIMITS,
    );
    expect(layout.kind).toBe("scroll");
    // the third column is not short (200 > 100): text at its natural 200 (below the cap)
    expect(widthsOf(layout)).toEqual([90, 330, 200, 330]);
  });

  it("gives a single word wider than the column its own width (a scroll, never a broken word)", () => {
    const widths = widthsOf(
      screenLayout(
        [
          { min: 50, max: 60 },
          { min: 900, max: 900 },
        ],
        600,
        LIMITS,
      ),
    );
    expect(widths).toEqual([60, 900]);
  });

  it("keeps the CJK floor of a column that may break after each character", () => {
    // min = the cell floor (5em + padding); max = the whole sentence on one line
    const widths = widthsOf(
      screenLayout(
        [
          { min: 80, max: 96 },
          { min: 108, max: 1600 },
          { min: 108, max: 1400 },
        ],
        320,
        LIMITS,
      ),
    );
    expect(widths[0]).toBe(96);
    expect(widths[1]).toBeGreaterThanOrEqual(108);
    expect(widths[2]).toBeGreaterThanOrEqual(108);
    expect(sum(widths)).toBeCloseTo(320);
  });
});

describe("screen rules of a measured table", () => {
  it("lays out a fitted table from its widths and lets a natural one keep its own width", () => {
    expect(declarations(ruleBody(screen, `.md table.${SCREEN_TABLE}`))).toContain(
      "table-layout: fixed",
    );
    expect(declarations(ruleBody(screen, `.md .table-wrap.${NATURAL_TABLE}`))).toEqual([
      "width: fit-content",
      "max-width: 100%",
    ]);
    expect(declarations(ruleBody(screen, `.md .table-wrap.${NATURAL_TABLE} > table`))).toContain(
      "width: auto",
    );
  });
});

describe("rules of a table fitted for paper", () => {
  it("are paper's alone: an exported page read on a narrow screen wraps its tables as the browser does", () => {
    // the export keeps the paper class in its page and inlines the reading styles: they must not fix its layout
    expect(codeOnly(screen)).not.toContain(FIT_TABLE);
    expect(codeOnly(paper)).not.toContain(SCREEN_TABLE);
    const print = codeOnly(paper).indexOf("@media print");
    expect(print).toBeGreaterThanOrEqual(0);
    expect(codeOnly(paper).indexOf(FIT_TABLE)).toBeGreaterThan(print);
  });
});

/** A table of `html` whose cells are `width(cell)` wide at max-content, half that at min-content (jsdom has no layout). */
function laidOut(html: string, width: (cell: HTMLTableCellElement) => number): HTMLTableElement {
  const table = document.createElement("table");
  table.innerHTML = html;
  document.body.append(table);
  for (const cell of table.querySelectorAll<HTMLTableCellElement>("td, th")) {
    cell.getBoundingClientRect = () => {
      const full = width(cell);
      return new DOMRect(0, 0, table.style.width === "min-content" ? full / 2 : full, 20);
    };
  }
  return table;
}

describe("measureColumns", () => {
  const byText = (cell: HTMLTableCellElement): number => (cell.textContent ?? "").length * 10;

  it("measures each column at max-content and min-content width, a pixel more", () => {
    const table = laidOut("<tr><td>abcd</td><td>ab</td></tr><tr><td>a</td><td>b</td></tr>", byText);
    expect(measureColumns(table)).toEqual([
      { min: 21, max: 41 },
      { min: 11, max: 21 },
    ]);
    expect(table.style.width).toBe("");
  });

  it("leaves a table with a cell spanning columns | rows, or an uneven row, to the browser", () => {
    const colspan = laidOut(
      '<tr><td colspan="2">ab</td></tr><tr><td>a</td><td>b</td></tr>',
      byText,
    );
    const rowspan = laidOut(
      '<tr><td rowspan="2">ab</td><td>c</td></tr><tr><td>d</td></tr>',
      byText,
    );
    const ragged = laidOut("<tr><td>a</td><td>b</td></tr><tr><td>c</td></tr>", byText);
    expect(measureColumns(colspan)).toBeNull();
    expect(measureColumns(rowspan)).toBeNull();
    expect(measureColumns(ragged)).toBeNull();
  });
});
