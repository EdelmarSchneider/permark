// The switcher bar's rules, pure: when it shows, which workspace comes next.

import type { WindowView, WorkspaceSummary } from "../../api";

/** The bar shows only when there is something to switch between. */
export function barShows(list: readonly WorkspaceSummary[]): boolean {
  return list.length >= 2;
}

/**
 * The workspace `step` places after `current` (negative = before), wrapping around; `current` = the one this window
 * shows (`null`: it shows none: next = the first, previous = the last). `null` when there is nothing to go to.
 */
export function neighbor(
  list: readonly WorkspaceSummary[],
  current: number | null,
  step: 1 | -1,
): WorkspaceSummary | null {
  if (!barShows(list)) return null;
  const at = list.findIndex((w) => w.id === current);
  const to = at < 0 ? (step === 1 ? 0 : list.length - 1) : (at + step + list.length) % list.length;
  return list[to] ?? null;
}

/** The workspace a window shows (what stays on screen while another opens), `null` when it shows none. */
export function shownId(view: WindowView): number | null {
  switch (view.state) {
    case "showing":
      return view.workspace.id;
    case "opening":
      return view.showing?.id ?? null;
    case "welcome":
    case "closing":
      return null;
    default:
      return view satisfies never;
  }
}
