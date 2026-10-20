// The leave guard: anything that takes the window away from the document being edited (another document, closing
// the workspace or the window) asks first. The editing session installs the guard while it holds a document;
// without one, leaving is always fine.

import type { Leave } from "./docMachine";

type Guard = (leave: Leave) => Promise<boolean>;

let guard: Guard | null = null;

/** Installs (or removes, `null`) the guard; returns its removal. */
export function setLeaveGuard(next: Guard | null): () => void {
  guard = next;
  return () => {
    if (guard === next) guard = null;
  };
}

/** Whether the caller may go on (unsaved edits saved or discarded, or none). */
export function confirmLeave(leave: Leave): Promise<boolean> {
  return guard === null ? Promise.resolve(true) : guard(leave);
}
