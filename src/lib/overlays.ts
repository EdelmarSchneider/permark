// A tiny stack of open overlays (menus, popovers, dialogs, quick open, the find bar). Esc closes the
// topmost one; Follow checks whether any is open before it jumps to another document.

type Entry = { id: number; close: () => void };

const stack: Entry[] = [];
let nextId = 1;

/** Registers an open overlay. Returns the function that unregisters it (call it when the overlay closes). */
export function registerOverlay(close: () => void): () => void {
  const entry: Entry = { id: nextId++, close };
  stack.push(entry);
  return () => {
    const i = stack.indexOf(entry);
    if (i >= 0) stack.splice(i, 1);
  };
}

/** Closes the topmost overlay. Returns false when none is open. */
export function closeTopOverlay(): boolean {
  const top = stack[stack.length - 1];
  if (!top) return false;
  top.close();
  return true;
}

export function hasOpenOverlay(): boolean {
  return stack.length > 0;
}

/**
 * Remembers the focused element and returns a function that puts the focus back on it
 * (Esc closes an overlay and the focus returns to where it was).
 */
export function rememberFocus(): () => void {
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  return () => {
    if (previous && previous.isConnected && previous !== document.body) previous.focus();
  };
}
