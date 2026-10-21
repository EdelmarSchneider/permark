// Esc closes the review (the Done button says so). Innermost first: a field, a menu, a dialog, an overlay, the image
// zoom, the find bar and an open editor keep their Escape; focus mode leaves before (its handler runs first).

import { ownedByTarget } from "../../core/doc/escape";

export type EscapeEvent = {
  key: string;
  isComposing: boolean;
  defaultPrevented: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  target: EventTarget | null;
};

/** What else may want the Escape now. */
export type EscapeEnv = {
  reviewing: boolean;
  /** An editor shows the document (Esc goes back to View first). */
  editing: boolean;
  /** Search everywhere | the palette | a question owns the keyboard. */
  modal: boolean;
  /** The picture zoom is open. */
  zoomed: boolean;
  /** The find bar is open. */
  finding: boolean;
  /** A dialog | menu | pop-up is shown somewhere (it closes on Esc wherever the keyboard is). */
  overlay: boolean;
};

/** Whether this key press closes the review. Pure. */
export function closesReview(event: EscapeEvent, env: EscapeEnv): boolean {
  if (event.key !== "Escape" || event.isComposing || event.defaultPrevented) return false;
  if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return false;
  if (!env.reviewing || env.editing || env.modal || env.zoomed || env.finding || env.overlay)
    return false;
  return !ownedByTarget(event.target);
}
