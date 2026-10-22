// Wrapped lines of a very long text, counted without drawing them. The viewer draws such a text in a monospace font
// and breaks a long line after any character (a terminal's way, `line-break: anywhere`), so the rows a line takes
// follow from its width in columns: the first row holds `cols` columns, every further row `cols - INDENT - lead`
// (it starts a little to the right of the line's own indentation, `lead`). Only the lines near the screen are drawn;
// these numbers place them. Pure.
//
// Why not at spaces, as smaller files wrap: the browser also breaks after `-`, `/` and other signs, by rules a count
// here cannot follow exactly; a count that is off would cut the last rows of the file. Breaking anywhere is exact.

/** Continuation rows start this many columns to the right (of the line's indentation). */
export const INDENT = 2;
/** Fewest columns a row is counted with (a very narrow window still makes progress). */
export const MIN_COLS = INDENT + 8;
/** Columns between tab stops (the CSS default the viewer keeps). */
export const TAB = 8;

const PLAIN = /^[\x20-\x7e]*$/;

/** A character drawn two columns wide in a monospace font (East Asian wide | full width, most emoji). */
function wide(code: number): boolean {
  return (
    (code >= 0x1100 && code <= 0x115f) ||
    (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f) ||
    (code >= 0xac00 && code <= 0xd7a3) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0xfe30 && code <= 0xfe4f) ||
    (code >= 0xff00 && code <= 0xff60) ||
    (code >= 0xffe0 && code <= 0xffe6) ||
    (code >= 0x1f300 && code <= 0x1f64f) ||
    (code >= 0x1f900 && code <= 0x1f9ff) ||
    (code >= 0x20000 && code <= 0x3fffd)
  );
}

/** A character that takes no column (a combining mark, a zero-width one). */
function zero(code: number): boolean {
  return (
    (code >= 0x0300 && code <= 0x036f) ||
    (code >= 0x200b && code <= 0x200f) ||
    (code >= 0xfe00 && code <= 0xfe0f) ||
    code === 0xfeff
  );
}

/** The columns a line takes (a close count: fonts differ for rare characters; the drawing never overlaps). */
export function columns(line: string): number {
  return measure(line).width;
}

/** The columns a line takes and the columns of its leading spaces | tabs. */
export function measure(line: string): { width: number; lead: number } {
  if (PLAIN.test(line)) return { width: line.length, lead: line.length - line.trimStart().length };
  let width = 0;
  let lead = -1;
  for (const char of line) {
    const code = char.codePointAt(0) ?? 0;
    const blank = code === 0x09 || code === 0x20;
    if (!blank && lead < 0 && code !== 0x0d) lead = width;
    if (code === 0x09) width += TAB - (width % TAB);
    else if (code === 0x0d) continue;
    else if (zero(code)) continue;
    else width += wide(code) ? 2 : 1;
  }
  return { width, lead: lead < 0 ? width : lead };
}

/** The most indentation the further rows of a line keep (a deeply indented line still gets room). */
export const MAX_LEAD = 24;

/** The indentation a line's further rows keep: its own, at most `MAX_LEAD` and half of what a row holds. */
export function leadFor(lead: number, cols: number): number {
  const c = Math.max(cols, MIN_COLS);
  return Math.max(0, Math.min(lead, MAX_LEAD, Math.floor((c - INDENT) / 2)));
}

/** The rows a line of `width` columns (indented `lead`) takes in rows of `cols` columns. */
export function rowsOf(width: number, cols: number, lead = 0): number {
  const c = Math.max(cols, MIN_COLS);
  if (width <= c) return 1;
  return 1 + Math.ceil((width - c) / (c - INDENT - leadFor(lead, c)));
}

/** Where each line starts, in rows: `starts[i]` = rows above line i; `starts[n]` = all the rows. */
export function rowStarts(
  widths: ArrayLike<number>,
  cols: number,
  leads?: ArrayLike<number>,
): Float64Array {
  const starts = new Float64Array(widths.length + 1);
  let at = 0;
  for (let i = 0; i < widths.length; i++) {
    starts[i] = at;
    at += rowsOf(widths[i] ?? 0, cols, leads?.[i] ?? 0);
  }
  starts[widths.length] = at;
  return starts;
}

/** The line holding row `row` (0-based) and which of its rows it is; past the end: the last line's last row. */
export function lineAtRow(starts: Float64Array, row: number): { line: number; row: number } {
  const lines = starts.length - 1;
  if (lines <= 0) return { line: 0, row: 0 };
  const total = starts[lines] ?? 0;
  const r = Math.min(Math.max(0, Math.floor(row)), Math.max(0, total - 1));
  // the last line whose start is at | above `r`
  let lo = 0;
  let hi = lines - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if ((starts[mid] ?? 0) <= r) lo = mid;
    else hi = mid - 1;
  }
  return { line: lo, row: r - (starts[lo] ?? 0) };
}

/** Where row `row` of a line starts, in characters (one column each: the long lines this is used for are plain). */
export function rowOffset(row: number, cols: number, lead = 0): number {
  const c = Math.max(cols, MIN_COLS);
  return row <= 0 ? 0 : c + (row - 1) * (c - INDENT - leadFor(lead, c));
}

/** Rows `from` .. `to` (exclusive) of a line: its text from where `from` starts, never cutting a surrogate pair. */
export function rowsText(line: string, from: number, to: number, cols: number, lead = 0): string {
  const [start, end] = rowsRange(line, from, to, cols, lead);
  return line.slice(start, end);
}

/** Where rows `from` .. `to` (exclusive) of a line start and end, in characters (`rowsText`'s cut). */
export function rowsRange(
  line: string,
  from: number,
  to: number,
  cols: number,
  lead = 0,
): [number, number] {
  return [edge(line, rowOffset(from, cols, lead)), edge(line, rowOffset(to, cols, lead))];
}

/** `at`, moved back off the second half of a surrogate pair. */
function edge(line: string, at: number): number {
  if (at <= 0) return 0;
  if (at >= line.length) return line.length;
  const code = line.charCodeAt(at);
  return code >= 0xdc00 && code <= 0xdfff ? at - 1 : at;
}

/** A piece of the text to draw: line `line` (0-based), its rows `from` .. `to`. */
export type Piece = { line: number; from: number; to: number };

/**
 * The pieces that cover `count` rows from row `first`. A line of at most `whole` rows is drawn whole (from its first
 * row, even when only its end is in view); a longer one only by the rows asked for. `top` = the row the first piece
 * starts at.
 */
export function piecesFor(
  starts: Float64Array,
  first: number,
  count: number,
  whole: number,
): { top: number; pieces: Piece[] } {
  const lines = starts.length - 1;
  if (lines <= 0 || count <= 0) return { top: 0, pieces: [] };
  const at = lineAtRow(starts, first);
  const pieces: Piece[] = [];
  let left = count;
  let top = starts[at.line] ?? 0;
  for (let line = at.line; line < lines && left > 0; line++) {
    const rows = (starts[line + 1] ?? 0) - (starts[line] ?? 0);
    const skip = line === at.line ? at.row : 0;
    if (rows > whole) {
      const to = Math.min(rows, skip + left);
      if (line === at.line) top += skip;
      pieces.push({ line, from: skip, to });
      left -= to - skip;
    } else {
      pieces.push({ line, from: 0, to: rows });
      left -= rows - skip;
    }
  }
  return { top, pieces };
}

/** The columns and indentation of every line. */
export type Widths = { widths: Int32Array; leads: Int32Array };

/** The columns of every line, the ones `before` had for the same lines kept (a text that grew by a chunk). */
export function widthsOf(
  lines: readonly string[],
  before: ({ lines: readonly string[] } & Widths) | null,
): Widths {
  const widths = new Int32Array(lines.length);
  const leads = new Int32Array(lines.length);
  let from = 0;
  // a chunk goes on with the last line held: every line before it is the same string
  if (before !== null && before.lines.length > 1 && lines.length >= before.lines.length) {
    const kept = before.lines.length - 1;
    if (lines[0] === before.lines[0] && lines[kept - 1] === before.lines[kept - 1]) {
      widths.set(before.widths.subarray(0, kept));
      leads.set(before.leads.subarray(0, kept));
      from = kept;
    }
  }
  for (let i = from; i < lines.length; i++) {
    const m = measure(lines[i] ?? "");
    widths[i] = m.width;
    leads[i] = m.lead;
  }
  return { widths, leads };
}

// ---- the line kept at the top when the rows change (wrapping switched, a new width) ----------------------------

/** A line and how far into its first row the pane starts (pixels; never past that row). */
export type Anchor = { line: number; delta: number };

/**
 * The line at the top of rows of one height `row` (no wrapping): the line holding the pane's top edge `y` (pixels
 * from the first line's top). A line showing less than a pixel is passed over: rounding never makes the line above
 * the one being read the anchor.
 */
export function anchorAt(y: number, row: number, count: number): Anchor {
  const line = Math.min(Math.max(1, Math.floor((y + 1) / row) + 1), Math.max(count, 1));
  return { line, delta: Math.max(0, y - (line - 1) * row) };
}

/** How far into the anchored line the pane starts again: within its first row (the person read from there). */
export function intoLine(delta: number, row: number): number {
  return Math.max(0, Math.min(delta, row - 1));
}
