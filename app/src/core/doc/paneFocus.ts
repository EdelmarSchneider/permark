// The document pane (`DocumentArea.svelte`, `[data-scroller]`) draws its focus ring only while it holds a keyboard
// sent to it on purpose (a side hidden, a link followed). A keyboard that fell back on it because what held it went
// from under it (a review ended, a comment field closed) draws none: the person never left the page, and a ring
// around the whole page would only flash there.

/** The attribute that lets the pane draw its ring (read by `DocumentArea.svelte`). */
export const SENT = "data-focus-sent";

/** The keyboard goes to `pane` on purpose: it shows so until the pane loses the keyboard inside the window (the
 *  window going to the background keeps it). */
export function sendToPane(pane: HTMLElement): void {
  pane.focus({ preventScroll: true });
  if (document.activeElement !== pane) return;
  pane.setAttribute(SENT, "");
  const lost = (): void => {
    if (!document.hasFocus()) return;
    pane.removeAttribute(SENT);
    pane.removeEventListener("blur", lost);
  };
  pane.addEventListener("blur", lost);
}
