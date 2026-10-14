// `data-sourcepos="12:1-15:3"` on top-level blocks lets a search hit (which carries a start line) find its block.

export type LineRange = { startLine: number; endLine: number };

export function parseSourcepos(value: string | null | undefined): LineRange | null {
  if (!value) return null;
  const m = /^(\d+):\d+-(\d+):\d+$/.exec(value.trim());
  if (!m) return null;
  return { startLine: Number(m[1]), endLine: Number(m[2]) };
}

/**
 * Index of the block that covers `line`; when the line falls in a gap (blank lines between blocks),
 * the closest block before it. Null when no block has position data.
 */
export function blockForLine(ranges: (LineRange | null)[], line: number): number | null {
  let before: number | null = null;
  for (let i = 0; i < ranges.length; i++) {
    const r = ranges[i];
    if (!r) continue;
    if (line >= r.startLine && line <= r.endLine) return i;
    if (r.startLine <= line) before = i;
  }
  return before;
}
