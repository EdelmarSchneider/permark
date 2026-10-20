// Which rows of a long table are drawn: the ones near the screen, at a fixed row height (pure).

/** Height of a row of a long table (px). */
export const ROW_HEIGHT = 30;
/** Tables with this many rows or more draw only the rows near the screen. */
export const VIRTUAL_FROM = 200;
/** Rows drawn above and below the screen. */
const OVERSCAN = 20;

/**
 * The rows to draw: `offset` = how far the top of the screen is below the first row (negative: the table starts
 * lower on the screen), `height` = the screen's height.
 */
export function windowOf(
  offset: number,
  height: number,
  total: number,
): { first: number; count: number } {
  const top = Math.floor(Math.max(0, offset) / ROW_HEIGHT);
  const first = Math.max(0, Math.min(top - OVERSCAN, total - 1));
  const count = Math.ceil(Math.max(0, height) / ROW_HEIGHT) + 2 * OVERSCAN;
  return { first, count };
}
