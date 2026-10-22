// Live updates off: the project's folder cannot report its changes (a network drive, WSL, out of watches), so
// PerMark cannot follow them as they happen. One signal in the status bar: the Live chip shows it when the chip is
// there (it claims the signal); else the core's own item does. Single files and the Notes never follow live.

import { untrack } from "svelte";
import { conditions } from "../conditions/context.svelte";

class LiveOffSignal {
  #claims = $state(0);

  /** Live updates are off in this project (reactive). */
  get off(): boolean {
    const now = conditions.current;
    return now.workspace === "project" && now.liveUpdates.status === "unavailable";
  }

  /** A status item shows the off state itself: the core's item steps aside (reactive). */
  get claimed(): boolean {
    return this.#claims > 0;
  }

  /** Claims the signal until the returned function runs (call it from an effect of the item; the count is not
   *  tracked there, so the effect does not run again on its own write). */
  claim(): () => void {
    this.#claims = untrack(() => this.#claims) + 1;
    let released = false;
    return () => {
      if (released) return;
      released = true;
      this.#claims = untrack(() => this.#claims) - 1;
    };
  }
}

export const liveOff = new LiveOffSignal();
