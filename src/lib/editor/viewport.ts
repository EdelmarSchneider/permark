// What the Outline needs to know about an editor's scrolling: that the view moved, and whether the person did it.

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
