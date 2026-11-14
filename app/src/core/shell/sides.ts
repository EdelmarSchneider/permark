// Showing | hiding the regions of this window (Mod+B, Mod+Alt+B, Mod+J, the status bar toggles, the outline toggle)
// and showing | selecting | moving | closing a view in whatever region it sits: the stores' state read, the pure step
// of `layout.ts` applied to the view's own region, the frame animated, the focus let go of a region that hides (its
// controls go inert: keys reach the page).

import { tick } from "svelte";
import type { WindowView, WorkspaceView } from "../../api";
import { reading } from "../doc/reading.svelte";
import { windowStore } from "../window.svelte";
import {
  regionAvailable,
  toggleOutline,
  togglePanel as panelStep,
  toggleRegion as regionStep,
  type RegionChoice,
  type RegionState,
} from "./layout";
import { layout } from "./layout.svelte";
import { moveTo, OUTLINE, SEARCH, select, type Arrangement, type Region } from "./arrangement";
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

/** `region` as the stores have it now (reactive when read in a `$derived`): the views arranged in it, the person's
 *  choices. */
export function regionState(region: Region): RegionState {
  const here = (id: string): boolean => arrangement.regionOf(id) === region;
  return {
    // at the left the Notes' own list too (not a view)
    fixed:
      (region === "left" && windowStore.screen?.workspace.kind === "notes") ||
      arrangement
        .stacked(region)
        .some((group) => group.views.some((id) => arrangement.viewOf(id)?.closable === false)),
    hasOutline: arrangement.hasOutline && here(OUTLINE),
    outlineOn: reading.outline,
    outlineSelected: arrangement.isSelected(OUTLINE),
    // the panels the person opens | closes
    panels: arrangement.views.flatMap(({ id, closable, panel }) =>
      closable && panel !== null && here(id)
        ? [{ id, open: arrangement.isOpen(panel), selected: arrangement.isSelected(id) }]
        : [],
    ),
    // a bottom region the window is too short to draw is off like one the person hid
    hidden: isHidden(region) || (region === "bottom" && layout.bottomFit === null),
  };
}

/** The region the view `id` is arranged in (one no region lists: the right, where a panel opens by default). */
function regionWith(id: string): Region {
  return arrangement.regionOf(id) ?? "right";
}

/** The state of the region the view `id` is arranged in (reactive when read in a `$derived`). */
export function regionStateOf(id: string): RegionState {
  return regionState(regionWith(id));
}

/**
 * `region` has its toggle now (the status bar's button, its command; reactive when read in a `$derived`). The left
 * and the bottom region: while they are there (a present view; at the left the Notes' list too). The right one also
 * while it only could show something: its toggle then turns the outline on, else opens the first panel.
 */
export function hasToggle(region: Region): boolean {
  switch (region) {
    case "left":
      return hasSidebar(windowStore.view);
    case "right":
      return regionAvailable(regionState(region));
    case "bottom":
      return arrangement.presentIn(region) > 0;
    default:
      return region satisfies never;
  }
}

/** A region about to hide gives the keyboard focus to the page (it goes inert): the editor when one is open, else
 *  the document's scroller (arrows | Page keys scroll it), else the page region. */
function letGo(region: Region): void {
  if (typeof document === "undefined") return;
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return;
  if (active.closest(`[data-region="${REGION_NAME[region]}"]`) !== null) focusPage();
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

/** The keyboard goes to the page: the editor when one is open, else the document scroller, else the page region
 *  (no page: what holds it lets go). */
function focusPage(): void {
  const page = pageRegion();
  const target =
    page?.querySelector<HTMLElement>("[contenteditable='true'], .cm-content") ??
    documentPane() ??
    page;
  if (target) target.focus({ preventScroll: true });
  else if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
}

/** The first control of the view `id` as drawn (`null`: none, or the view is not drawn). */
function firstControl(id: string): HTMLElement | null {
  const controls = ':is(button:not([disabled]), [tabindex="0"], input, a[href])';
  return document.querySelector<HTMLElement>(`[data-view-slot="${id}"] ${controls}`);
}

/**
 * The keyboard goes to the view `id`, as drawn now: its options button (in its own header, or at the end of its tab
 * strip while it is the tab shown), else its tab, else its first control. A view that cannot take it (not drawn, its
 * region hidden): the page. Never the page body.
 */
export function focusView(id: string): void {
  const target =
    document.querySelector<HTMLElement>(`[data-view-menu="${id}"]`) ??
    document.querySelector<HTMLElement>(`[role="tab"][data-view="${id}"]`) ??
    firstControl(id);
  target?.focus();
  if (target === null || document.activeElement !== target) focusPage();
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

function apply(region: Region, choice: RegionChoice): void {
  if (choice.hidden !== undefined) setHidden(region, choice.hidden);
  if (choice.outlineOn !== undefined) reading.setOutline(choice.outlineOn);
  if (choice.open !== undefined) layout.setOpen(choice.open.id, choice.open.open);
  if (choice.select !== undefined) selectView(choice.select);
}

/** The toggle of `region` (its key, its status bar button), where the region has one: it hides | shows; the right
 *  region with nothing present turns the outline on, else opens the first panel arranged there. */
export function toggleRegion(region: Region): void {
  if (hasToggle(region)) apply(region, regionStep(regionState(region)));
}

/** Mod+B: the sidebar hides | shows (only where there is one). */
export function toggleSidebar(): void {
  toggleRegion("left");
}

/** Mod+Alt+B: the right panel hides | shows. */
export function toggleRightPanel(): void {
  toggleRegion("right");
}

/** Mod+J: the bottom panel hides | shows (only while a view is present there). */
export function toggleBottomPanel(): void {
  toggleRegion("bottom");
}

/** Mod+Alt+O, the header's outline button: the outline shows | hides (its region, when hidden, shows again with it). */
export function toggleOutlinePanel(): void {
  const region = regionWith(OUTLINE);
  apply(region, toggleOutline(regionState(region)));
}

/** A panel's toggle (its chip, its command): on screen -> closed; open elsewhere -> brought forward; closed -> open. */
export function togglePanel(id: string): void {
  const region = regionWith(id);
  apply(region, panelStep(regionState(region), id));
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

/**
 * Puts a view where a move decided (`next`: the arrangement with the view at its new place): drawn at once, the view
 * shown by its group; a region the person hid shows again with it. A view nobody offers now stays where it is.
 */
export function placeView(id: string, next: Arrangement): void {
  if (arrangement.viewOf(id) === undefined) return;
  arrangement.arrange(next);
  const region = arrangement.regionOf(id);
  if (region !== null) setHidden(region, false);
  selectView(id);
}

/** Moves a view to another region: the last tab of its last group (an empty region: its first group). */
export function moveToRegion(id: string, region: Region): void {
  placeView(id, moveTo(arrangement.layout, id, region));
}

/**
 * Closes a view (its place in the arrangement stays): a panel closed, the outline off. `held`: the keyboard was on
 * that view from outside its body (its options menu, opened from its tab strip).
 */
export function closeView(id: string, held = false): void {
  const active = typeof document === "undefined" ? null : document.activeElement;
  const inside =
    active instanceof HTMLElement && active.closest(`[data-view-slot="${id}"]`) !== null;
  const region = arrangement.regionOf(id);
  if (id === OUTLINE) reading.setOutline(false);
  else layout.setOpen(id, false);
  // the keyboard was on that view: once it is gone, the next control of its region takes it (else the page); never
  // the page body
  if (inside || held) void tick().then(() => keepFocus(region));
}

/** After a view closed under the focus: the first control of a view its region still shows, else the page. */
function keepFocus(region: Region | null): void {
  const now = document.activeElement;
  if (now instanceof HTMLElement && now !== document.body && now.isConnected) return;
  for (const group of region === null ? [] : arrangement.drawn(region)) {
    const next = firstControl(arrangement.selectedOf(group) ?? "");
    if (next) {
      next.focus();
      return;
    }
  }
  focusPage();
}
