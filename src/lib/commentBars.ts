// Where the margin bars of the comments go (docs/design/v0.4.md section 3). A comment knows the lines of its
// passage; the rendered document has blocks that know theirs. A bar spans the innermost blocks that touch the
// comment's lines. Pure: the document view measures the blocks and draws what this returns.

/** A block of the rendered document: its lines in the source, how deep it sits in the page, and where it is on screen. */
export type BlockBox = {
  startLine: number;
  endLine: number;
  /** Nesting depth in the rendered document (a list item inside a list is deeper than the list). */
  depth: number;
  /** Vertical extent, in the same coordinates for every block. */
  top: number;
  bottom: number;
};

export type Bar = { top: number; height: number };

/** A bar needs at least this much height to be seen. */
export const MIN_BAR_HEIGHT = 12;

const touches = (box: BlockBox, start: number, end: number): boolean =>
  box.startLine <= end && box.endLine >= start;

/** `inner` lies inside `outer`: its lines are within, and it is smaller or deeper. */
const inside = (inner: BlockBox, outer: BlockBox): boolean =>
  inner !== outer &&
  inner.startLine >= outer.startLine &&
  inner.endLine <= outer.endLine &&
  (inner.startLine !== outer.startLine ||
    inner.endLine !== outer.endLine ||
    inner.depth > outer.depth);

/**
 * The bar for lines `start` to `end`: from the top of the first block that holds the passage to the bottom of the last,
 * where a block that holds another touching block is left out in favour of the inner one (a comment on one list item does
 * not span the whole list). `null` when no block shows those lines.
 */
export function barFor(boxes: readonly BlockBox[], start: number, end: number): Bar | null {
  const last = Math.max(start, end);
  const hit = boxes.filter((box) => touches(box, start, last));
  const innermost = hit.filter((box) => !hit.some((other) => inside(other, box)));
  if (innermost.length === 0) return null;
  const top = Math.min(...innermost.map((box) => box.top));
  const bottom = Math.max(...innermost.map((box) => box.bottom));
  return { top, height: Math.max(MIN_BAR_HEIGHT, bottom - top) };
}
