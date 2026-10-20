// Showing | hiding the two side panels of this window (Mod+B, Mod+Alt+B, the status bar toggles, the outline toggle):
// the stores' state read, the pure step of `layout.ts` applied, the frame animated, the focus let go of a region that
// hides (its controls go inert: keys reach the page).

import type { WindowView } from "../../api";
import { documents } from "../doc/document.svelte";
import { reading } from "../doc/reading.svelte";
import { windowStore } from "../window.svelte";
import { toggleOutline, toggleRight, type RightChoice, type RightSide } from "./layout";
import { layout } from "./layout.svelte";
import { activeRight } from "./panels.svelte";
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
  const view = documents.view;
  return {
    panel: activeRight(),
    hasOutline: view?.body.kind === "markdown" && view.body.outline.length > 0,
    outlineOn: reading.outline,
    hidden: layout.rightHidden,
  };
}

/** A region about to hide gives up the keyboard focus (it goes inert). */
function letGo(region: "sidebar" | "right"): void {
  if (typeof document === "undefined") return;
  const active = document.activeElement;
  if (active instanceof HTMLElement && active.closest(`[data-region="${region}"]`) !== null) {
    active.blur();
  }
}

function apply(choice: RightChoice): void {
  if (choice.hidden === layout.rightHidden && choice.outlineOn === reading.outline) return;
  ui.animateFrame();
  if (choice.hidden) letGo("right");
  layout.setRightHidden(choice.hidden);
  reading.setOutline(choice.outlineOn);
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
