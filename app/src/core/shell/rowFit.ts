// How many same-size buttons a row can show beside a part that must stay (a whole heading, the tab shown at its
// smallest); the rest move into a menu whose button needs room too. Pure; the row measures itself and asks.

export type RowRoom = {
  /** Px the row has for its content. */
  readonly room: number;
  /** Px of what always stays (a label at the least it keeps and the buttons that never move out, with their gaps). */
  readonly fixed: number;
  /** Buttons that can move into the menu. */
  readonly count: number;
  /** Px each of them takes, with its gap. */
  readonly step: number;
  /** Px the menu button takes, with its gap, once one of them is in it. */
  readonly more: number;
};

/** The buttons shown, from the first: all while they fit, else as many as fit beside the menu button. A row not
 *  measured yet (no room, nothing kept) shows them all, so the first frame never folds. */
export function rowFit({ room, fixed, count, step, more }: RowRoom): number {
  if (!(room > 0) || !(fixed > 0)) return count;
  if (fixed + count * step <= room) return count;
  const shown = Math.floor((room - fixed - more) / step);
  return Math.max(0, Math.min(count - 1, shown));
}
