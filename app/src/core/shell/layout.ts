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

/** A right panel offered now: whether it is open, and whether it is the view its group shows (selected). */
export type SidePanel = { readonly id: string; readonly open: boolean; readonly selected: boolean };

/** What decides the right region: the document's outline, the panels plugins offer, the person's choices. */
export type RightSide = {
  /** The document shown has an outline. */
  hasOutline: boolean;
  /** The outline is on (`reading.outline`). */
  outlineOn: boolean;
  /** The outline is the view its group shows (not a tab behind another). */
  outlineSelected: boolean;
  /** The right panels offered now, in column order. */
  panels: readonly SidePanel[];
  /** The person hid the right panel. */
  hidden: boolean;
};

/** The right region has something it could show (a toggle there would change the screen). */
export function rightAvailable(side: RightSide): boolean {
  return side.hasOutline || side.panels.length > 0;
}

/** Some view is present: the outline (when on), an open panel. */
function anyPresent(side: RightSide): boolean {
  return (side.hasOutline && side.outlineOn) || side.panels.some((p) => p.open);
}

/** The right region is on screen: not hidden, with a present view. */
export function rightShown(side: RightSide): boolean {
  return !side.hidden && anyPresent(side);
}

/** The outline is on screen now. */
export function outlineShown(side: RightSide): boolean {
  return !side.hidden && side.hasOutline && side.outlineOn && side.outlineSelected;
}

/** The panel `id` is on screen now. */
export function panelShown(side: RightSide, id: string): boolean {
  const panel = side.panels.find((p) => p.id === id);
  return !side.hidden && panel !== undefined && panel.open && panel.selected;
}

/**
 * The person's choices after a step (absent = unchanged): the region hidden, the outline on, a panel opened | closed,
 * the view selected in its group.
 */
export type RightChoice = {
  hidden?: boolean;
  outlineOn?: boolean;
  open?: { id: string; open: boolean };
  select?: string;
};

/** The right-panel toggle: shown -> hidden; hidden -> shown; nothing present -> the outline turns on (else the first
 *  offered panel opens); nothing it could show -> no change. */
export function toggleRight(side: RightSide): RightChoice {
  if (rightShown(side)) return { hidden: true };
  if (!rightAvailable(side)) return {};
  if (anyPresent(side)) return { hidden: false };
  if (side.hasOutline) return { hidden: false, outlineOn: true, select: "outline" };
  const first = side.panels[0];
  return first === undefined
    ? {}
    : { hidden: false, open: { id: first.id, open: true }, select: first.id };
}

/** The outline toggle: on screen -> off; else on, selected in its group, the region shown. */
export function toggleOutline(side: RightSide): RightChoice {
  if (outlineShown(side)) return { outlineOn: false };
  return { hidden: false, outlineOn: true, select: "outline" };
}

/** A panel's toggle: on screen -> closed; open but not on screen -> selected + shown (never closed); closed -> open. */
export function togglePanel(side: RightSide, id: string): RightChoice {
  if (panelShown(side, id)) return { open: { id, open: false } };
  const panel = side.panels.find((p) => p.id === id);
  if (panel?.open === true) return { hidden: false, select: id };
  return { hidden: false, open: { id, open: true }, select: id };
}
