// Showing | hiding the regions of this window (Mod+B, Mod+Alt+B, the status bar toggles, the outline toggle) and
// showing | selecting | closing a view in whatever region it sits: the stores' state read, the pure step of
// `layout.ts` applied, the frame animated, the focus let go of a region that hides (its controls go inert: keys
// reach the page).

import { tick } from "svelte";
import type { WindowView, WorkspaceView } from "../../api";
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
import { OUTLINE, SEARCH, select, type Region } from "./arrangement";
import { arrangement } from "./arrangement.svelte";
import { ui } from "./ui.svelte";

/** The name the frame gives each region (`data-region`). */
const REGION_NAME: Readonly<Record<Region, string>> = {
  left: "sidebar",
  right: "right",
  bottom: "bottom",
};

/** A workspace of this kind has something at the left: the Notes their own list, any other a present view arranged
 *  there (a project: its files; a single file: only what the person put there). */
function leftOf(kind: WorkspaceView["kind"]): boolean {
  return kind === "notes" || arrangement.drawn("left").length > 0;
}

/** The window has a left region now (reactive when read in a `$derived`): its toggle and Mod+B apply. */
export function hasSidebar(view: WindowView): boolean {
  switch (view.state) {
    case "showing":
      return leftOf(view.workspace.kind);
    case "opening":
      return view.showing !== null && leftOf(view.showing.kind);
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
    hasOutline: arrangement.hasOutline,
    outlineOn: reading.outline,
    outlineSelected: arrangement.isSelected(OUTLINE),
    // the panels the person opens | closes (those of the right side)
    panels: arrangement.views.flatMap(({ id, closable, panel }) =>
      closable && panel !== null
        ? [{ id, open: arrangement.isOpen(panel), selected: arrangement.isSelected(id) }]
        : [],
    ),
    hidden: layout.rightHidden,
  };
}

/** A region about to hide gives the keyboard focus to the page (it goes inert): the editor when one is open, else
 *  the document's scroller (arrows | Page keys scroll it), else the page region. */
function letGo(region: Region): void {
  if (typeof document === "undefined") return;
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return;
  if (active.closest(`[data-region="${REGION_NAME[region]}"]`) !== null) focusPage(active);
}

/** The person hid `region`. */
function isHidden(region: Region): boolean {
  switch (region) {
    case "left":
      return layout.sidebarHidden;
    case "right":
      return layout.rightHidden;
    case "bottom":
      return layout.bottomHidden;
    default:
      return region satisfies never;
  }
}

/** `region` hides | shows (a change only): the frame animates, a region that hides lets the focus go. */
function setHidden(region: Region, hidden: boolean): void {
  if (hidden === isHidden(region)) return;
  ui.animateFrame();
  if (hidden) letGo(region);
  switch (region) {
    case "left":
      layout.setSidebarHidden(hidden);
      break;
    case "right":
      layout.setRightHidden(hidden);
      break;
    case "bottom":
      layout.setBottomHidden(hidden);
      break;
    default:
      region satisfies never;
  }
}

/** The page region of this window (`null`: not drawn). */
function pageRegion(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[data-region="page"]');
}

/** The pane the document scrolls in: all of the document that is drawn, and nothing of the window around it
 *  (`null`: no page on screen). */
export function documentPane(): HTMLElement | null {
  return pageRegion()?.querySelector<HTMLElement>("[data-scroller]") ?? null;
}

/** The keyboard goes to the page: the editor when one is open, else the document scroller, else the page region. */
function focusPage(active: HTMLElement): void {
  const page = pageRegion();
  const target =
    page?.querySelector<HTMLElement>("[contenteditable='true'], .cm-content") ??
    documentPane() ??
    page;
  if (target) target.focus({ preventScroll: true });
  else active.blur();
}

/** `id` becomes the view its group shows (its group: every view of its arrangement group). */
export function selectView(id: string): void {
  const region = arrangement.regionOf(id);
  const group =
    region === null ? undefined : arrangement.layout[region].find((g) => g.views.includes(id));
  layout.setSelected(select(layout.views.selected, group?.views ?? [id], id));
  // the Search brought forward (its tab, its command): its box takes the cursor
  if (id === SEARCH) ui.searchFocus += 1;
}

function apply(choice: RightChoice): void {
  if (choice.hidden !== undefined) setHidden("right", choice.hidden);
  if (choice.outlineOn !== undefined) reading.setOutline(choice.outlineOn);
  if (choice.open !== undefined) layout.setOpen(choice.open.id, choice.open.open);
  if (choice.select !== undefined) selectView(choice.select);
}

/** Mod+B: the sidebar hides | shows (only where there is one). */
export function toggleSidebar(): void {
  if (hasSidebar(windowStore.view)) setHidden("left", !layout.sidebarHidden);
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

/**
 * Shows a view, in whatever region it sits: the region shown, the view selected in its group; one the person can
 * close is opened too (the outline turned on). A view that is always open has no choice to remember; one nobody
 * offers now (its plugin off) changes nothing.
 */
export function showView(id: string): void {
  const view = arrangement.viewOf(id);
  if (view === undefined) return;
  setHidden(arrangement.regionOf(id) ?? view.region, false);
  if (view.closable) {
    if (view.panel === null) reading.setOutline(true);
    else layout.setOpen(id, true);
  }
  selectView(id);
}

/** Closes a view (its place in the arrangement stays): a panel closed, the outline off. */
export function closeView(id: string): void {
  const active = typeof document === "undefined" ? null : document.activeElement;
  const inside =
    active instanceof HTMLElement && active.closest(`[data-view-slot="${id}"]`) !== null;
  const region = arrangement.regionOf(id);
  if (id === OUTLINE) reading.setOutline(false);
  else layout.setOpen(id, false);
  // the keyboard was in that view: once it is gone, the next control of its region takes it (else the page); never
  // the page body
  if (inside && active instanceof HTMLElement) {
    const was = active;
    void tick().then(() => keepFocus(was, region));
  }
}

/** After a view closed under the focus: the first control of a view its region still shows, else the page. */
function keepFocus(was: HTMLElement, region: Region | null): void {
  const now = document.activeElement;
  if (now instanceof HTMLElement && now !== document.body && now.isConnected) return;
  const controls = ':is(button:not([disabled]), [tabindex="0"], input, a[href])';
  for (const group of region === null ? [] : arrangement.drawn(region)) {
    const shown = arrangement.selectedOf(group) ?? "";
    const next = document.querySelector<HTMLElement>(`[data-view-slot="${shown}"] ${controls}`);
    if (next) {
      next.focus();
      return;
    }
  }
  focusPage(was);
}
