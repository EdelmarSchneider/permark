// What the Outline needs to know about an editor's scrolling (that the view moved, and whether the person did it), and which block is at
// the top of a view, for the modes to keep the place between them.

/**
 * Calls `notify(false)` when `scroller` scrolls and `notify(true)` when the person takes over scrolling or the caret (wheel,
 * touch, a key, a pointer press), which ends the "section I just chose" pin. Returns the function that stops listening.
 */
export function watchViewport(
  scroller: HTMLElement,
  notify: (byUser: boolean) => void,
): () => void {
  const scrolled = (): void => notify(false);
  const took = (): void => notify(true);
  const options: AddEventListenerOptions = { passive: true };
  scroller.addEventListener("scroll", scrolled, options);
  const takers = ["wheel", "touchmove", "keydown", "pointerdown"] as const;
  for (const name of takers) scroller.addEventListener(name, took, options);
  return () => {
    scroller.removeEventListener("scroll", scrolled);
    for (const name of takers) scroller.removeEventListener(name, took);
  };
}

/** A line's worth of a block: showing this much of it at the top of the view makes it the block being looked at. */
const TOP_VISIBLE_PX = 24;

/**
 * Whether a block that spans `top` to `bottom` (screen coordinates) is the one at the top of a view whose upper edge is at `edge`:
 * a line's worth of it shows (half of it, for a block shorter than two lines). A block cut off so that only its last few pixels
 * show is the one that was scrolled past, so the mode that opens next does not start on it.
 */
export function showsAtTop(top: number, bottom: number, edge: number): boolean {
  const visible = bottom - Math.max(top, edge);
  return visible > 0 && visible >= Math.min(TOP_VISIBLE_PX, (bottom - top) / 2);
}
