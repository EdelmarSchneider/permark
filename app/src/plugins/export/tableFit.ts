// Column widths of a table too wide for the paper, from its measured content. Paper cannot scroll and its cells may
// break inside a word, so the browser's own sharing squeezes a column of short words down to a letter or two while a
// long text column keeps most of the room. Here short columns stay whole, long text columns share the rest
// by their length and wrap between words; a word breaks only when it alone is wider than any fair share.

/** A column's narrowest width without breaking a word, and its width on one line (border box, pixels). */
export type ColumnSpan = { min: number; max: number };

/** The class of a table laid out from the widths set here (fixed layout on paper). */
export const FIT_TABLE = "pm-fit-table";

const total = (values: readonly number[]): number => values.reduce((sum, value) => sum + value, 0);

/** Widths that fill `room` exactly; null when every column fits on one line (the browser's layout is fine). */
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

/**
 * The column spans of `table` as the page lays it out now (it must be in the document), or null when a cell spans
 * columns | rows (the widths of its own columns cannot be told apart).
 */
export function measureColumns(table: HTMLTableElement): ColumnSpan[] | null {
  const rows = Array.from(table.rows);
  const first = Array.from(rows[0]?.cells ?? []);
  const spans = rows.some((row) =>
    Array.from(row.cells).some((cell) => cell.colSpan > 1 || cell.rowSpan > 1),
  );
  if (first.length === 0 || spans) return null;
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

/** Gives `table` the widths as shares of its own width (so it fills the printable width whatever its scale). */
export function fitColumns(table: HTMLTableElement, widths: readonly number[]): void {
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
  table.classList.add(FIT_TABLE);
}
