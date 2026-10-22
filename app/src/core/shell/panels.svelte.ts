// The panels plugins contribute: tabs of the left sidebar (next to Files and Search) and the views of the right column.
// Which sidebar tab shows lives in `ui`; the right column's arrangement in `rightViews.svelte.ts`. A panel whose plugin
// went off | that its plugin does not offer now is not listed.

import { conditions } from "../conditions/context.svelte";
import { contributions } from "../plugins/instance.svelte";
import type { ContributionOf } from "../plugins/registry.svelte";
import { ui } from "./ui.svelte";

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

/** Tabs plugins add to the sidebar, in a fixed order (`stableOrder`). */
export function leftPanels(): Panel[] {
  return stableOrder(
    contributions
      .visible("panel", conditions.current)
      .filter((p) => p.side === "left" && isOffered(p)),
  );
}

/** Panels plugins add at the right of the page, in a fixed order (`stableOrder`). */
export function rightPanels(): Panel[] {
  return stableOrder(
    contributions
      .visible("panel", conditions.current)
      .filter((p) => p.side === "right" && isOffered(p)),
  );
}

/** The sidebar view that shows: a core one, or a plugin tab that still exists; else Files. */
export function activeSidebar(): string {
  if (ui.sidebar === "files" || ui.sidebar === "search") return ui.sidebar;
  return leftPanels().some((p) => p.id === ui.sidebar) ? ui.sidebar : "files";
}
