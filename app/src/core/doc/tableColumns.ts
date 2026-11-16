// Column widths of a table from its measured content, shared by the screen (reading view and Edit) and paper.
// The browser's own sharing gives most of the room to a long text column and squeezes a column of short words; here
// short columns stay whole, long text columns share the rest by their length and wrap between words.
// Paper cannot scroll and its cells may break inside a word: there a word breaks only when it alone is wider than
// any fair share. The screen keeps every word whole and scrolls the table in its wrapper as the last resort.

/** A column's narrowest width without breaking a word, and its width on one line (border box, pixels). */
export type ColumnSpan = { min: number; max: number };

/** The class of a table laid out for paper from the widths set here (fixed layout in print only: an exported page
 *  keeps it, and on a narrow screen it must wrap as any other table). */
export const FIT_TABLE = "pm-fit-table";
/** The class of a table laid out for the screen from the widths set here (fixed layout; set by the live app only). */
export const SCREEN_TABLE = "pm-screen-table";
/** The class of a table's wrapper when the table keeps its own width (well short of the column). */
export const NATURAL_TABLE = "is-natural";

const total = (values: readonly number[]): number => values.reduce((sum, value) => sum + value, 0);

/** Widths that fill `room` exactly; null when every column fits on one line (the browser's layout is fine). On paper
 *  a word may break when even the longest words do not fit; the screen scrolls first (`screenLayout`). */
export function columnWidths(columns: readonly ColumnSpan[], room: number): number[] | null {
  const min = columns.map((column) => column.min);
  const max = columns.map((column) => Math.max(column.min, column.max));
  if (total(max) <= room) return null;
  if (total(min) >= room) return capped(min, room);

  const width = max.map(() => 0);
  let open = columns.map((_, at) => at);
  let left = room;
  const floor = (ids: readonly number[]): number => total(ids.map((at) => min[at] ?? 0));
  // a column no wider than an equal share of what is left stays whole, unless the others would then lose a word
  for (;;) {
    const share = left / open.length;
    const short = open.find((at) => {
      const natural = max[at] ?? 0;
      return natural <= share && left - natural >= floor(open) - (min[at] ?? 0);
    });
    if (short === undefined) break;
    width[short] = max[short] ?? 0;
    left -= width[short];
    open = open.filter((at) => at !== short);
  }
  // the long columns share the rest by length, none below its longest word (`left` covers every open floor)
  for (;;) {
    const length = total(open.map((at) => max[at] ?? 0));
    const share = (at: number): number => (left * (max[at] ?? 0)) / length;
    const below = open.filter((at) => share(at) < (min[at] ?? 0));
    if (below.length === 0) {
      for (const at of open) width[at] = share(at);
      return width;
    }
    for (const at of below) {
      width[at] = min[at] ?? 0;
      left -= width[at];
    }
    open = open.filter((at) => !below.includes(at));
  }
}

/** Even the longest words do not fit: the narrow ones stay whole, the widest share what is left equally. */
function capped(min: readonly number[], room: number): number[] {
  const sorted = [...min].sort((a, b) => a - b);
  let left = room;
  let level = Infinity;
  for (const [at, value] of sorted.entries()) {
    const share = left / (sorted.length - at);
    if (value > share) {
      level = share;
      break;
    }
    left -= value;
  }
  return min.map((value) => Math.min(value, level));
}

/** Limits of a screen column (border box, pixels): `short` = its lines never wrap; `cap` = the widest long text
 *  column of a table that scrolls anyway. */
export type ScreenLimits = { short: number; cap: number };

/**
 * A table on screen: `auto` = every line fits and the table fills the column (the browser's own layout); `natural` =
 * every line fits well short of the column, the table keeps its own width; `fill` = widths that fill the column;
 * `scroll` = widths wider than the column (its wrapper scrolls).
 */
export type TableLayout =
  | { kind: "auto" }
  | { kind: "natural" }
  | { kind: "fill"; widths: number[] }
  | { kind: "scroll"; widths: number[] };

/**
 * Below this share of the column a table whose lines all fit keeps its own width: stretched out, a few short values
 * drift apart across the page; nearer the full width, filling it lines the frame up with the prose at little cost.
 */
export const NATURAL_SHARE = 0.7;

/** The layout of a table with these measured columns in `room` pixels (see `TableLayout`). */
export function screenLayout(
  columns: readonly ColumnSpan[],
  room: number,
  limits: ScreenLimits,
): TableLayout {
  const max = columns.map((column) => Math.max(column.min, column.max));
  const natural = total(max);
  if (natural <= room)
    return natural < room * NATURAL_SHARE ? { kind: "natural" } : { kind: "auto" };
  // a short column (numbers, dates, ids) keeps its whole line: its floor is its natural width
  const short = max.map((width) => width <= limits.short);
  const floors = columns.map((column, at) => (short[at] ? (max[at] ?? 0) : column.min));
  if (total(floors) <= room) {
    const spans = floors.map((min, at) => ({ min, max: max[at] ?? 0 }));
    const widths = columnWidths(spans, room);
    if (widths) return { kind: "fill", widths };
  }
  // the floors do not fit: the table scrolls anyway, so long text gets a readable width up to the cap
  const widths = columns.map((column, at) =>
    short[at] ? (max[at] ?? 0) : Math.max(column.min, Math.min(max[at] ?? 0, limits.cap)),
  );
  return { kind: "scroll", widths };
}

/**
 * The column spans of `table` as the page lays it out now (it must be in the document), or null when a cell spans
 * columns | rows or a row has more | fewer cells than the first (the widths of its own columns cannot be told apart).
 */
export function measureColumns(table: HTMLTableElement): ColumnSpan[] | null {
  const rows = Array.from(table.rows);
  const first = Array.from(rows[0]?.cells ?? []);
  const uneven = rows.some(
    (row) =>
      row.cells.length !== first.length ||
      Array.from(row.cells).some((cell) => cell.colSpan > 1 || cell.rowSpan > 1),
  );
  if (first.length === 0 || uneven) return null;
  // a whole pixel more: the fixed layout rounds the shares to its own sub-pixel units, and a column a fraction short
  // of its longest word breaks that word (the paper's cells may break anywhere)
  const widthsAt = (width: string): number[] => {
    table.style.width = width;
    return first.map((cell) => Math.ceil(cell.getBoundingClientRect().width) + 1);
  };
  const max = widthsAt("max-content");
  const min = widthsAt("min-content");
  table.style.removeProperty("width");
  return first.map((_, at) => ({ min: min[at] ?? 0, max: max[at] ?? 0 }));
}

/** Gives `table` the widths as shares of its own width (so it fills its room whatever its scale), marked with
 *  `kind` (`FIT_TABLE` for paper, `SCREEN_TABLE` for the screen). */
export function fitColumns(
  table: HTMLTableElement,
  widths: readonly number[],
  kind: typeof FIT_TABLE | typeof SCREEN_TABLE,
): void {
  const sum = total(widths);
  if (sum <= 0) return;
  for (const old of table.querySelectorAll(":scope > colgroup")) old.remove();
  const group = document.createElement("colgroup");
  for (const width of widths) {
    const col = document.createElement("col");
    col.style.width = `${Number(((width / sum) * 100).toFixed(3))}%`;
    group.append(col);
  }
  table.prepend(group);
  table.classList.add(kind);
}
