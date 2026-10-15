// The right panel's views (docs/design/v0.2.md section 0, v0.4.md section 3): a segmented switch between Changes, Outline
// and Notes, built so another view can be added to the list. The choice is remembered per project (the session's `rightTab`).

export const RIGHT_TABS = ["changes", "outline", "notes"] as const;
export type RightTab = (typeof RIGHT_TABS)[number];

export const DEFAULT_RIGHT_TAB: RightTab = "changes";

export function isRightTab(value: unknown): value is RightTab {
  return typeof value === "string" && (RIGHT_TABS as readonly string[]).includes(value);
}

/** The view to show for a stored session; a name this build does not know (a newer UI wrote it) is the default. */
export function normalizeRightTab(value: unknown): RightTab {
  return isRightTab(value) ? value : DEFAULT_RIGHT_TAB;
}

/** The neighbouring view, wrapping around (the arrow keys of the switch). */
export function stepTab(tab: RightTab, direction: 1 | -1): RightTab {
  const n = RIGHT_TABS.length;
  return RIGHT_TABS[(RIGHT_TABS.indexOf(tab) + direction + n) % n];
}

export type PanelView = {
  /** The panel is open (not collapsed). */
  open: boolean;
  tab: RightTab;
  /** The view to go back to when the shortcut is pressed again. */
  back: RightTab;
};

/**
 * Ctrl+Alt+O: show the Outline (opening the panel if it is collapsed); when the Outline is already showing,
 * go back to the view that was there before.
 */
export function toggleOutline(view: PanelView): PanelView {
  if (view.open && view.tab === "outline") {
    const back = view.back === "outline" ? DEFAULT_RIGHT_TAB : view.back;
    return { open: true, tab: back, back };
  }
  return { open: true, tab: "outline", back: view.tab === "outline" ? view.back : view.tab };
}
