// Dragging rows of the Explorer with pointer events. HTML5 drag and drop is not used: inside the Tauri webview the
// window's own file drop takes the drag over on Windows, so a row would never be dropped on another one.
//
// A press becomes a drag once the pointer has moved a few pixels (a click stays a click). While it lasts the tracker
// asks the caller where a drop would land, opens a closed folder the pointer rests on, scrolls the list when the
// pointer is near its edge, and ends on release or Esc.

/** How far (px) the pointer moves before a press is a drag. */
const THRESHOLD = 5;
/** How long (ms) the pointer rests on a closed folder before it opens. */
export const EXPAND_DELAY_MS = 600;
/** The strip (px) at the top and bottom of the list in which it scrolls. */
const EDGE = 28;
const SCROLL_STEP = 14;

export type DragHooks = {
  /** The drag begins (the pointer moved far enough). */
  begin(): void;
  /** The pointer moved: `folder` is where a drop would land, or null when it would land nowhere valid. */
  over(target: { folder: string | null; x: number; y: number }): void;
  /** The pointer rested on a closed folder: open it. */
  expand(path: string): void;
  /** The drag is over: `folder` is where it was released, null when nothing valid or it was cancelled. */
  end(folder: string | null): void;
  /** The element under the pointer decides where a drop lands (null for nowhere). */
  folderAt(element: Element | null): string | null;
  /** The scrolling list, moved when the pointer is near its top or bottom edge. */
  scroller(): HTMLElement | null;
};

/** A closed folder row under an element, if there is one. */
function closedFolderAt(element: Element | null): string | null {
  const row = element?.closest<HTMLElement>("[data-path]");
  if (!row || row.dataset.dir !== "true" || row.getAttribute("aria-expanded") !== "false") {
    return null;
  }
  return row.dataset.path ?? null;
}

/**
 * Follows a pressed pointer until it is released. Call it from `pointerdown`; it listens on the window and
 * removes its listeners when the press ends.
 */
export function trackPointerDrag(start: PointerEvent, hooks: DragHooks): void {
  const origin = { x: start.clientX, y: start.clientY };
  let dragging = false;
  let folder: string | null = null;
  let resting: string | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const rest = (path: string | null): void => {
    if (path === resting) return;
    resting = path;
    if (timer !== null) clearTimeout(timer);
    timer = null;
    if (path !== null) {
      timer = setTimeout(() => {
        timer = null;
        if (dragging && resting === path) hooks.expand(path);
      }, EXPAND_DELAY_MS);
    }
  };

  const scrollNearEdge = (y: number): void => {
    const list = hooks.scroller();
    if (!list) return;
    const box = list.getBoundingClientRect();
    if (y < box.top + EDGE) list.scrollTop -= SCROLL_STEP;
    else if (y > box.bottom - EDGE) list.scrollTop += SCROLL_STEP;
  };

  const finish = (target: string | null): void => {
    window.removeEventListener("pointermove", onMove, true);
    window.removeEventListener("pointerup", onUp, true);
    window.removeEventListener("pointercancel", onCancel, true);
    window.removeEventListener("keydown", onKey, true);
    if (timer !== null) clearTimeout(timer);
    if (dragging) hooks.end(target);
  };

  function onMove(e: PointerEvent): void {
    if (!dragging) {
      if (Math.hypot(e.clientX - origin.x, e.clientY - origin.y) < THRESHOLD) return;
      dragging = true;
      hooks.begin();
    }
    e.preventDefault();
    const element = document.elementFromPoint(e.clientX, e.clientY);
    folder = hooks.folderAt(element);
    hooks.over({ folder, x: e.clientX, y: e.clientY });
    rest(closedFolderAt(element));
    scrollNearEdge(e.clientY);
  }

  function onUp(): void {
    finish(folder);
  }

  function onCancel(): void {
    finish(null);
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key !== "Escape") return;
    // Esc cancels the drag and nothing else
    e.preventDefault();
    e.stopPropagation();
    finish(null);
  }

  window.addEventListener("pointermove", onMove, true);
  window.addEventListener("pointerup", onUp, true);
  window.addEventListener("pointercancel", onCancel, true);
  window.addEventListener("keydown", onKey, true);
}
