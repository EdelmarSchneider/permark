// Panel widths: limits, clamping and fitting into the window. Pure; the store is `layout.svelte.ts`.

export type Range = { readonly min: number; readonly max: number; readonly def: number };

export const SIDEBAR: Range = { min: 200, max: 480, def: 260 };
export const RIGHT: Range = { min: 220, max: 560, def: 280 };
/** The page always keeps at least this much. */
export const PAGE_MIN = 360;
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
