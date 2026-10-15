// Which build of the app this is, asked of the backend once. The Microsoft Store updates the app it installs, so in the
// Store build the interface has no "Check for updates" (see `updates.svelte.ts`, `commands.ts` and the Settings popover),
// and the agent setup lines name the `trailmark` alias (the backend sends that as the program).

import { api } from "../api";
import type { Distribution } from "../types";

class DistributionStore {
  /** "standard" until the backend answers, and when it cannot: the build the project publishes. */
  value = $state<Distribution>("standard");

  private asked: Promise<void> | null = null;

  get isStore(): boolean {
    return this.value === "store";
  }

  /** Asks the backend the first time; every caller waits for that one answer. Never rejects. */
  load(): Promise<void> {
    this.asked ??= api.appDistribution().then(
      (value) => {
        this.value = value;
      },
      () => {
        // the default stands: a build that cannot say what it is acts like the standard one
      },
    );
    return this.asked;
  }
}

export const distribution = new DistributionStore();
