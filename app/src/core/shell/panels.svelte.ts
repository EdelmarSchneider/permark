// The panels plugins contribute: views of the window beside the core's own (Files, Search, the outline). A panel's
// side is where it goes by default; where each view sits (its region and group) and which one a group shows live in
// `arrangement.svelte.ts`. A panel whose plugin went off | that its plugin does not offer now is not listed.

import { conditions } from "../conditions/context.svelte";
import { contributions } from "../plugins/instance.svelte";
import type { ContributionOf } from "../plugins/registry.svelte";

type Panel = ContributionOf<"panel">;

/**
 * Panels in a fixed order: by owning plugin id, then as that plugin contributed them. Plugins load in parallel, so the
 * order they finish in (= registration order) differs between windows and launches.
 */
export function stableOrder(panels: readonly Panel[]): Panel[] {
  return panels
    .map((panel, index) => ({ panel, index }))
    .sort((a, b) =>
      a.panel.owner < b.panel.owner ? -1 : a.panel.owner > b.panel.owner ? 1 : a.index - b.index,
    )
    .map(({ panel }) => panel);
}

/** The panel is drawn now: its plugin offers it (a dormant one waits until the project calls for it). */
export function isOffered(panel: Panel): boolean {
  return panel.offered?.() ?? true;
}

/** The panels plugins offer now, of both sides, in a fixed order (`stableOrder`). */
export function offeredPanels(): Panel[] {
  return stableOrder(contributions.visible("panel", conditions.current).filter(isOffered));
}
