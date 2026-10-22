// Showing | hiding the two side panels of this window (Mod+B, Mod+Alt+B, the status bar toggles, the outline toggle):
// the stores' state read, the pure step of `layout.ts` applied, the frame animated, the focus let go of a region that
// hides (its controls go inert: keys reach the page).

import type { WindowView } from "../../api";
import { reading } from "../doc/reading.svelte";
import { windowStore } from "../window.svelte";
import {
  toggleOutline,
  togglePanel as panelStep,
  toggleRight,
  type RightChoice,
  type RightSide,
} from "./layout";
import { layout } from "./layout.svelte";
import { OUTLINE, select } from "./rightViews";
import { rightViews } from "./rightViews.svelte";
import { ui } from "./ui.svelte";

/** Workspaces with a sidebar: a project, the Notes. */
export function hasSidebar(view: WindowView): boolean {
  switch (view.state) {
    case "showing":
      return view.workspace.kind === "project" || view.workspace.kind === "notes";
    case "opening":
      return view.showing?.kind === "project" || view.showing?.kind === "notes";
    case "welcome":
    case "closing":
      return false;
    default:
      return view satisfies never;
  }
}

/** The right region as the stores have it now (reactive when read in a `$derived`). */
export function rightSide(): RightSide {
  return {
    hasOutline: rightViews.hasOutline,
    outlineOn: reading.outline,
    outlineSelected: rightViews.isSelected(OUTLINE),
    panels: rightViews.panels.map((panel) => ({
      id: panel.id,
      open: rightViews.isOpen(panel),
      selected: rightViews.isSelected(panel.id),
    })),
    hidden: layout.rightHidden,
  };
}

/** A region about to hide gives the keyboard focus to the page (it goes inert): the editor when one is open, else
 *  the document's scroller (arrows | Page keys scroll it), else the page region. */
function letGo(region: "sidebar" | "right"): void {
  if (typeof document === "undefined") return;
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || active.closest(`[data-region="${region}"]`) === null) {
    return;
  }
  focusPage(active);
}

/** The keyboard goes to the page: the editor when one is open, else the document scroller, else the page region. */
function focusPage(active: HTMLElement): void {
  const page = document.querySelector<HTMLElement>('[data-region="page"]');
  const target =
    page?.querySelector<HTMLElement>("[contenteditable='true'], .cm-content") ??
    page?.querySelector<HTMLElement>("[data-scroller]") ??
    page;
  if (target) target.focus({ preventScroll: true });
  else active.blur();
}

/** `id` becomes the view its group shows (its group: every view of its arrangement group). */
export function selectView(id: string): void {
  const group = rightViews.layout.groups.find((g) => g.views.includes(id));
  layout.setSelected(select(layout.views.selected, group?.views ?? [id], id));
}

function apply(choice: RightChoice): void {
  if (choice.hidden !== undefined && choice.hidden !== layout.rightHidden) {
    ui.animateFrame();
    if (choice.hidden) letGo("right");
    layout.setRightHidden(choice.hidden);
  }
  if (choice.outlineOn !== undefined) reading.setOutline(choice.outlineOn);
  if (choice.open !== undefined) layout.setOpen(choice.open.id, choice.open.open);
  if (choice.select !== undefined) selectView(choice.select);
}

/** Mod+B: the sidebar hides | shows (only where there is one). */
export function toggleSidebar(): void {
  if (!hasSidebar(windowStore.view)) return;
  const hide = !layout.sidebarHidden;
  ui.animateFrame();
  if (hide) letGo("sidebar");
  layout.setSidebarHidden(hide);
}

/** Mod+Alt+B: the right panel hides | shows. */
export function toggleRightPanel(): void {
  apply(toggleRight(rightSide()));
}

/** Mod+Alt+O, the header's outline button: the outline shows | hides (a hidden right panel shows again with it). */
export function toggleOutlinePanel(): void {
  apply(toggleOutline(rightSide()));
}

/** A panel's toggle (its chip, its command): on screen -> closed; open elsewhere -> brought forward; closed -> open. */
export function togglePanel(id: string): void {
  apply(panelStep(rightSide(), id));
}

/** Shows a view of the right column: opened (the outline turned on), selected in its group, the region shown. */
export function showView(id: string): void {
  apply(
    id === OUTLINE
      ? { hidden: false, outlineOn: true, select: id }
      : { hidden: false, open: { id, open: true }, select: id },
  );
}

/** Closes a view of the right column (its place in the arrangement stays): a panel closed, the outline off. */
export function closeView(id: string): void {
  // the keyboard was in that view: it goes to another view's options, else to the page
  if (typeof document !== "undefined") {
    const active = document.activeElement;
    if (active instanceof HTMLElement && active.closest(`[data-right-view="${id}"]`) !== null) {
      const other = [...document.querySelectorAll<HTMLElement>("[data-view-menu]")].find(
        (el) => el.dataset["viewMenu"] !== id,
      );
      if (other) other.focus();
      else focusPage(active);
    }
  }
  if (id === OUTLINE) reading.setOutline(false);
  else layout.setOpen(id, false);
}
