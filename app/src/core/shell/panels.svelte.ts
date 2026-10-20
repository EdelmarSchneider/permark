// The panels plugins contribute: tabs of the left sidebar (next to Files and Search) and the panel at the right of the
// page. Which one shows lives in `ui`; a panel whose plugin went off is not shown (the view falls back).

import { conditions } from "../conditions/context.svelte";
import { contributions } from "../plugins/instance.svelte";
import type { ContributionOf } from "../plugins/registry.svelte";
import { ui } from "./ui.svelte";

type Panel = ContributionOf<"panel">;

/** Tabs plugins add to the sidebar, in the order they were contributed. */
export function leftPanels(): Panel[] {
  return contributions.visible("panel", conditions.current).filter((p) => p.side === "left");
}

/** Panels plugins add at the right of the page. */
export function rightPanels(): Panel[] {
  return contributions.visible("panel", conditions.current).filter((p) => p.side === "right");
}

/** The sidebar view that shows: a core one, or a plugin tab that still exists; else Files. */
export function activeSidebar(): string {
  if (ui.sidebar === "files" || ui.sidebar === "search") return ui.sidebar;
  return leftPanels().some((p) => p.id === ui.sidebar) ? ui.sidebar : "files";
}

/** The plugin panel at the right, if one is chosen and exists. */
export function activeRight(): string | null {
  const wanted = ui.right;
  return wanted !== null && rightPanels().some((p) => p.id === wanted) ? wanted : null;
}
