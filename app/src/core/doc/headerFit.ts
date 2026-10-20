// How much of the document header fits its width. Pure; the header measures itself and asks.

/**
 * `full`: every button in the row. `fold`: secondary actions (recent places, reveal, copy reference, open in editor,
 * find) and the saved time move into the "…" menu. `compact`: also the mode switch + Save show as icons and the role
 * badge moves into the menu.
 */
export type HeaderFit = "full" | "fold" | "compact";

/** Widths (px) from which a level fits with View | Edit | Source only; `extra` adds what edit modes show. */
export const FULL_AT = 720;
export const FOLD_AT = 460;
/** Extra width of the Save button and of the diagram toggle (Source of Markdown). */
export const SAVE_PX = 70;
export const DIAGRAM_PX = 30;

/** The level for a header `width` px wide; `extra` = px of edit controls shown (Save, diagram toggle). A width not
 *  measured yet (0, not finite) = `full`, so the first frame never flashes the compact row. */
export function headerFit(width: number, extra: number): HeaderFit {
  if (!Number.isFinite(width) || width <= 0) return "full";
  const more = Number.isFinite(extra) && extra > 0 ? extra : 0;
  if (width >= FULL_AT + more) return "full";
  if (width >= FOLD_AT + more) return "fold";
  return "compact";
}

/** The px the edit controls add to the header in this state. */
export function editExtra(save: boolean, diagram: boolean): number {
  return (save ? SAVE_PX : 0) + (diagram ? DIAGRAM_PX : 0);
}
