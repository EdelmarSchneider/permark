// Whether the person can be looking at this view: its window in front and visible, and the view the one the window
// draws (reading counts only then). A tab hidden behind another is a child webview that keeps `visibilityState ===
// "visible"` and may keep the focus, so the tab list decides which view is on screen.

import { tabs } from "./shell/tabs.svelte";
import { windowStore } from "./window.svelte";

/** The view is the one its window draws: the window's Welcome (no tab), or a tab that is the active one. */
export function viewShown(tab: number | null, active: number | null): boolean {
  return tab === null || tab === active;
}

/** This view is the one its window draws (reactive). */
export const onScreen = (): boolean => viewShown(windowStore.tab, tabs.active);

export class InFront {
  /** The window is visible and has the focus. */
  #focused = $state(true);
  readonly #shown: () => boolean;

  constructor(shown: () => boolean = onScreen) {
    this.#shown = shown;
  }

  /** The person can be looking at this view (reactive). */
  get value(): boolean {
    return this.#focused && this.#shown();
  }

  /** Follows the window until the returned function runs; call it from an effect: `$effect(() => front.start())`. */
  start(): () => void {
    const sync = (): void => {
      this.#focused = document.visibilityState === "visible" && document.hasFocus();
    };
    sync();
    window.addEventListener("focus", sync);
    window.addEventListener("blur", sync);
    document.addEventListener("visibilitychange", sync);
    return () => {
      window.removeEventListener("focus", sync);
      window.removeEventListener("blur", sync);
      document.removeEventListener("visibilitychange", sync);
    };
  }
}
