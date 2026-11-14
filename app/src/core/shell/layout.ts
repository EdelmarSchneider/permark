// Panel sizes (the side panels' widths, the bottom panel's height): limits, clamping and fitting into the window; what
// a show | hide toggle of a region does. Pure; the store is `layout.svelte.ts`.

export type Range = { readonly min: number; readonly max: number; readonly def: number };

export const SIDEBAR: Range = { min: 200, max: 480, def: 260 };
export const RIGHT: Range = { min: 220, max: 560, def: 280 };
/** The bottom panel's height. Its maximum only bounds a saved value: what is drawn is cut by the window (`fitBottom`). */
export const BOTTOM: Range = { min: 120, max: 2000, def: 240 };
/** The page keeps at least this much while the panels can still give way: room for the document header folded
 *  (`doc/headerFit.ts`) and a readable column, so the panels yield before the page gets cramped. */
export const PAGE_MIN = 560;
/** The page keeps at least this height over the bottom panel: its header and a few lines of the document. */
export const PAGE_MIN_H = 220;
/** One arrow key press, and with Shift. */
export const STEP = 16;
export const STEP_BIG = 64;

/** A panel's size (a width, the bottom panel's height) inside its range (a number that is not finite = the default). */
export function clampWidth(range: Range, value: number): number {
  if (!Number.isFinite(value)) return range.def;
  return Math.min(range.max, Math.max(range.min, Math.round(value)));
}

/** A panel gives way by up to `over` px, never under its minimum: what it keeps, and what is still over. */
function giveWay(size: number | null, range: Range, over: number): [number | null, number] {
  if (size === null || over <= 0) return [size, over];
  const give = Math.min(over, size - range.min);
  return [size - give, over - give];
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
  const side = sidebar === null ? null : clampWidth(SIDEBAR, sidebar);
  const panel = right === null ? null : clampWidth(RIGHT, right);
  const [fitRight, over] = giveWay(panel, RIGHT, (side ?? 0) + (panel ?? 0) + PAGE_MIN - total);
  const [fitSidebar] = giveWay(side, SIDEBAR, over);
  return { sidebar: fitSidebar, right: fitRight };
}

/**
 * The bottom panel's height that fits `total` pixels of body height: the page keeps [`PAGE_MIN_H`], the panel gives
 * way down to its minimum; `null` when even that does not fit (the panel is not drawn) or there is no bottom panel.
 * A body height not known yet leaves the height as asked.
 */
export function fitBottom(total: number, bottom: number | null): number | null {
  if (bottom === null || !Number.isFinite(total) || total <= 0) return bottom;
  const asked = clampWidth(BOTTOM, bottom);
  const [height, over] = giveWay(asked, BOTTOM, asked + PAGE_MIN_H - total);
  return over > 0 ? null : height;
}

/** The way a panel grows from the edge its splitter sits on: the sidebar to the right, the right panel to the left,
 *  the bottom panel up. */
export type Grow = "right" | "left" | "up";

/** The arrow keys of a splitter: the one that makes its panel larger, the one that makes it smaller. */
const ARROWS: Readonly<Record<Grow, { readonly more: string; readonly less: string }>> = {
  right: { more: "ArrowRight", less: "ArrowLeft" },
  left: { more: "ArrowLeft", less: "ArrowRight" },
  up: { more: "ArrowUp", less: "ArrowDown" },
};

/** The key a splitter understands, as a change of its panel's size (`null`: not its key; the arrows across its bar
 *  are not). */
export function keyWidth(
  key: string,
  shift: boolean,
  width: number,
  range: Range,
  grows: Grow,
): number | null {
  const step = shift ? STEP_BIG : STEP;
  switch (key) {
    case ARROWS[grows].more:
      return clampWidth(range, width + step);
    case ARROWS[grows].less:
      return clampWidth(range, width - step);
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

/** A panel offered now that the person opens | closes: whether it is open, and whether it is the view its group shows
 *  (selected). */
export type SidePanel = { readonly id: string; readonly open: boolean; readonly selected: boolean };

/** What decides one region (left | right | bottom): the views arranged in it, the person's choices. */
export type RegionState = {
  /** Something that is always open shows there: a view the person cannot close (Files, Search, a plugin's sidebar
   *  tab), the Notes' own list. */
  fixed: boolean;
  /** The outline is arranged there and the document shown has one. */
  hasOutline: boolean;
  /** The outline is on (`reading.outline`). */
  outlineOn: boolean;
  /** The outline is the view its group shows (not a tab behind another). */
  outlineSelected: boolean;
  /** The panels arranged there that are offered now, in the order of the window's views. */
  panels: readonly SidePanel[];
  /** The person hid the region. */
  hidden: boolean;
};

/** The region has something it could show (a toggle there would change the screen). */
export function regionAvailable(side: RegionState): boolean {
  return side.fixed || side.hasOutline || side.panels.length > 0;
}

/** Some view is present: one that is always open, the outline (when on), an open panel. */
function anyPresent(side: RegionState): boolean {
  return side.fixed || (side.hasOutline && side.outlineOn) || side.panels.some((p) => p.open);
}

/** The region is on screen: not hidden, with a present view. */
export function regionShown(side: RegionState): boolean {
  return !side.hidden && anyPresent(side);
}

/** The outline is on screen now. */
export function outlineShown(side: RegionState): boolean {
  return !side.hidden && side.hasOutline && side.outlineOn && side.outlineSelected;
}

/** The panel `id` is on screen now. */
export function panelShown(side: RegionState, id: string): boolean {
  const panel = side.panels.find((p) => p.id === id);
  return !side.hidden && panel !== undefined && panel.open && panel.selected;
}

/**
 * The person's choices after a step (absent = unchanged): the region hidden, the outline on, a panel opened | closed,
 * the view selected in its group.
 */
export type RegionChoice = {
  hidden?: boolean;
  outlineOn?: boolean;
  open?: { id: string; open: boolean };
  select?: string;
};

/** A region's toggle: shown -> hidden; hidden -> shown; nothing present -> the outline turns on (else the first
 *  offered panel opens); nothing it could show -> no change. */
export function toggleRegion(side: RegionState): RegionChoice {
  if (regionShown(side)) return { hidden: true };
  if (!regionAvailable(side)) return {};
  if (anyPresent(side)) return { hidden: false };
  if (side.hasOutline) return { hidden: false, outlineOn: true, select: "outline" };
  const first = side.panels[0];
  return first === undefined
    ? {}
    : { hidden: false, open: { id: first.id, open: true }, select: first.id };
}

/** The outline toggle: on screen -> off; else on, selected in its group, the region shown. */
export function toggleOutline(side: RegionState): RegionChoice {
  if (outlineShown(side)) return { outlineOn: false };
  return { hidden: false, outlineOn: true, select: "outline" };
}

/** A panel's toggle: on screen -> closed; open but not on screen -> selected + shown (never closed); closed -> open. */
export function togglePanel(side: RegionState, id: string): RegionChoice {
  if (panelShown(side, id)) return { open: { id, open: false } };
  const panel = side.panels.find((p) => p.id === id);
  if (panel?.open === true) return { hidden: false, select: id };
  return { hidden: false, open: { id, open: true }, select: id };
}
