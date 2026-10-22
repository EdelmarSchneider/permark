// When the person has read what a view shows (unread markers, the review's "last seen"): on screen and in front for
// `READ_MS`, or sooner once they scrolled the document themselves. A glimpse (a Follow jump, a tab passed through)
// is not a read; a scroll the app makes (a place restored, a jump to a change) is not the person's.

/** On screen and in front this long counts as read (ms). */
export const READ_MS = 3000;

/** A scroll of the document pane this soon after the person's input (wheel, touch, keys, its scrollbar) is theirs. */
const INPUT_MS = 1000;

const SCROLL_KEYS = new Set(["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "]);

const PANE = "[data-scroller]";

function inPane(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(PANE) !== null;
}

/** Keys typed into a field | an editor move a caret, not the page. */
function typing(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])') !==
      null
  );
}

/** Calls `onScroll` each time the person scrolls the document pane, until the returned teardown runs. */
export function personScrolls(onScroll: () => void, root: Document = document): () => void {
  let inputAt = -Infinity;
  const input = (event: Event): void => {
    if (inPane(event.target)) inputAt = Date.now();
  };
  const key = (event: KeyboardEvent): void => {
    if (SCROLL_KEYS.has(event.key) && !typing(event.target)) input(event);
  };
  // a press on the pane itself, not on its content: its scrollbar
  const press = (event: PointerEvent): void => {
    if (event.target instanceof Element && event.target.matches(PANE)) inputAt = Date.now();
  };
  const scroll = (event: Event): void => {
    const target = event.target;
    if (target instanceof Element && target.matches(PANE) && Date.now() - inputAt <= INPUT_MS) {
      onScroll();
    }
  };
  const options = { capture: true, passive: true } as const;
  root.addEventListener("wheel", input, options);
  root.addEventListener("touchmove", input, options);
  root.addEventListener("keydown", key, options);
  root.addEventListener("pointerdown", press, options);
  root.addEventListener("scroll", scroll, options);
  return () => {
    root.removeEventListener("wheel", input, options);
    root.removeEventListener("touchmove", input, options);
    root.removeEventListener("keydown", key, options);
    root.removeEventListener("pointerdown", press, options);
    root.removeEventListener("scroll", scroll, options);
  };
}
