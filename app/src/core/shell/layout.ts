// Panel widths: limits, clamping and fitting into the window; what a show | hide toggle of a side panel does. Pure; the
// store is `layout.svelte.ts`.

export type Range = { readonly min: number; readonly max: number; readonly def: number };

export const SIDEBAR: Range = { min: 200, max: 480, def: 260 };
export const RIGHT: Range = { min: 220, max: 560, def: 280 };
/** The page keeps at least this much while the panels can still give way: room for the document header folded
 *  (`doc/headerFit.ts`) and a readable column, so the panels yield before the page gets cramped. */
export const PAGE_MIN = 560;
/** One arrow key press, and with Shift. */
export const STEP = 16;
export const STEP_BIG = 64;

/** A width inside its range (a number that is not finite = the default). */
export function clampWidth(range: Range, value: number): number {
  if (!Number.isFinite(value)) return range.def;
  return Math.min(range.max, Math.max(range.min, Math.round(value)));
}

/**
 * The widths that fit `total` pixels: the page keeps [`PAGE_MIN`], the panels give way (the right one first, then the
 * sidebar) down to their minimums. A window too small for even the minimums keeps them (the page shrinks).
 */
export function fit(
  total: number,
  sidebar: number | null,
  right: number | null,
): { sidebar: number | null; right: number | null } {
  if (!Number.isFinite(total) || total <= 0) return { sidebar, right };
  let side = sidebar === null ? null : clampWidth(SIDEBAR, sidebar);
  let panel = right === null ? null : clampWidth(RIGHT, right);
  let over = (side ?? 0) + (panel ?? 0) + PAGE_MIN - total;
  if (over > 0 && panel !== null) {
    const give = Math.min(over, panel - RIGHT.min);
    panel -= give;
    over -= give;
  }
  if (over > 0 && side !== null) {
    side -= Math.min(over, side - SIDEBAR.min);
  }
  return { sidebar: side, right: panel };
}

/** The key a splitter understands, as a change of the width (`null`: not its key). `grow` = the panel grows to the
 *  right (the sidebar's edge) or to the left (the right panel's edge). */
export function keyWidth(
  key: string,
  shift: boolean,
  width: number,
  range: Range,
  growsRight: boolean,
): number | null {
  const step = shift ? STEP_BIG : STEP;
  switch (key) {
    case "ArrowRight":
      return clampWidth(range, width + (growsRight ? step : -step));
    case "ArrowLeft":
      return clampWidth(range, width + (growsRight ? -step : step));
    case "Home":
      return range.min;
    case "End":
      return range.max;
    case "Enter":
      return range.def;
    default:
      return null;
  }
}

/** What decides the right region: the plugin panel chosen (if any), the document's outline, the person's choices. */
export type RightSide = {
  /** The plugin panel chosen at the right (`null`: none). */
  panel: string | null;
  /** The document shown has an outline. */
  hasOutline: boolean;
  /** The outline is on (`reading.outline`). */
  outlineOn: boolean;
  /** The person hid the right panel. */
  hidden: boolean;
};

/** The right region has something it could show (a toggle there would change the screen). */
export function rightAvailable(side: RightSide): boolean {
  return side.panel !== null || side.hasOutline;
}

/** The right region is on screen: not hidden, with a plugin panel, else the outline when on. */
export function rightShown(side: RightSide): boolean {
  return !side.hidden && (side.panel !== null || (side.hasOutline && side.outlineOn));
}

/** The outline is what the right region shows now. */
export function outlineShown(side: RightSide): boolean {
  return rightShown(side) && side.panel === null && side.hasOutline && side.outlineOn;
}

/** The person's choices after a step; `closePanel`: the chosen plugin panel lets go (the outline shows instead). */
export type RightChoice = { hidden: boolean; outlineOn: boolean; closePanel: boolean };

/** The right-panel toggle: shown -> hidden; not shown -> shown (with nothing on, the outline turns on); nothing it
 *  could show -> no change. */
export function toggleRight(side: RightSide): RightChoice {
  if (rightShown(side)) return { hidden: true, outlineOn: side.outlineOn, closePanel: false };
  if (!rightAvailable(side)) {
    return { hidden: side.hidden, outlineOn: side.outlineOn, closePanel: false };
  }
  return {
    hidden: false,
    outlineOn: side.panel === null ? true : side.outlineOn,
    closePanel: false,
  };
}

/** The outline toggle = "show me the outline" unless it shows: it shows (a hidden right panel shows again, a chosen
 *  plugin panel lets go); shown -> off. */
export function toggleOutline(side: RightSide): RightChoice {
  if (outlineShown(side)) return { hidden: false, outlineOn: false, closePanel: false };
  // any panel choice lets go (also one whose plugin went: it would come back over the outline)
  return { hidden: false, outlineOn: true, closePanel: true };
}
