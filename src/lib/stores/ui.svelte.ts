// Transient UI state shared across components: sidebar view, overlays, find bar, drag state, flash feedback.

import { SvelteSet } from "svelte/reactivity";
import type { FindMode } from "../find";

const HINT_KEY = "trailmark.hint.initProject";

function readHintDismissed(): boolean {
  try {
    return localStorage.getItem(HINT_KEY) === "1";
  } catch {
    return false;
  }
}

class UiStore {
  sidebarView = $state<"explorer" | "search">("explorer");
  quickOpen = $state(false);
  settingsOpen = $state(false);
  initDialog = $state(false);
  find = $state({
    open: false,
    query: "",
    mode: "phrase" as FindMode,
    total: 0,
    index: -1,
    /** Bumped to move the focus into the find input (Ctrl+F while already open). */
    focusTick: 0,
    /** False when the bar was opened by a search hit and focus must stay in the results. */
    takeFocus: true,
  });
  drag = $state<{ count: number } | null>(null);
  /** A panel edge is being dragged (disables the collapse animation). */
  resizing = $state(false);
  /** The right panel is hidden and something changed: the activity bar shows a dot. */
  rightNotify = $state(false);
  zoomImage = $state<string | null>(null);
  followChip = $state.raw<{ from: string; to: string; n: number } | null>(null);
  /** The "keep settings with the project" hint was dismissed or used (remembered across projects). */
  hintDismissed = $state(readHintDismissed());
  /** Repo state whose banner the user dismissed; it returns when the state changes. */
  dismissedRepoState = $state<string | null>(null);
  searchFocusTick = $state(0);
  explorerFocusTick = $state(0);
  /** Id of the control showing its "Copied" check right now. */
  flashed = $state<string | null>(null);
  collapsedSections = new SvelteSet<string>();

  private flashTimer: ReturnType<typeof setTimeout> | null = null;

  dismissHint(): void {
    this.hintDismissed = true;
    try {
      localStorage.setItem(HINT_KEY, "1");
    } catch {
      // private mode: the hint simply returns next launch
    }
  }

  flash(id: string): void {
    this.flashed = id;
    if (this.flashTimer !== null) clearTimeout(this.flashTimer);
    this.flashTimer = setTimeout(() => {
      this.flashed = null;
    }, 1200);
  }

  toggleSection(id: string): void {
    if (this.collapsedSections.has(id)) this.collapsedSections.delete(id);
    else this.collapsedSections.add(id);
  }
}

export const ui = new UiStore();
