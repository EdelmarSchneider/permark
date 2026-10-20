// F7 | Shift+F7: the next | previous change of the open document. The plugins that know of changes contribute steps
// (`changeStep`); the step with the lowest `order` that has something to move to does it, the rest stay quiet.

import type { ContributionRegistry } from "../plugins/registry.svelte";

/** Runs the steps in order. True when one moved; a step that fails is skipped. */
export async function stepChange(
  registry: Pick<ContributionRegistry, "of">,
  direction: 1 | -1,
): Promise<boolean> {
  const steps = [...registry.of("changeStep")].sort((a, b) => a.order - b.order);
  for (const item of steps) {
    try {
      if (await item.step(direction)) return true;
    } catch {
      // a step that cannot move leaves the turn to the next one
    }
  }
  return false;
}
