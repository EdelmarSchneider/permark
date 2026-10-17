// Transient UI state shared across components: sidebar view, overlays, find bar, drag state, flash feedback.

import { SvelteSet } from "svelte/reactivity";
import type { FindMode } from "../find";
import type { Anchor } from "../components/Popover.svelte";
import type { RightTab } from "../rightPanel";
import type { Scope } from "../searchEverywhereModel";

const HINT_KEY = "permark.hint.initProject";

function readHintDismissed(): boolean {
  try {
    return localStorage.getItem(HINT_KEY) === "1";
  } catch {
    return false;
  }
}

class UiStore {
  sidebarView = $state<"explorer" | "search" | "tags">("explorer");
  /** The Search everywhere box (Ctrl+T, Ctrl+P, Ctrl+Shift+P): null while closed, else the scope it was asked to show. */
  searchBox = $state.raw<{ scope: Scope } | null>(null);
  /** The scope Ctrl+T opens: the one last picked with Tab or a click on the strip. Kept for the session. */
  searchScope = $state<Scope>("all");
  settingsOpen = $state(false);
  initDialog = $state(false);
  /** The "What do these letters mean?" popover of the git badges: where it is anchored, and which control opened it. */
  gitLegend = $state.raw<{ anchor: Anchor; from: "status" | "explorer" } | null>(null);
  /** The folder switcher (the menu on the project's name in the Explorer): null while closed, else where it is anchored. */
  folderSwitcher = $state.raw<{ anchor: Anchor } | null>(null);
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
  /** Feed events arrived while another view of the right panel was showing: a dot on the Changes tab. */
  changesNotify = $state(false);
  /** The view the Outline shortcut goes back to when pressed again (not remembered across launches). */
  rightTabBack = $state<RightTab>("changes");
  /** Bumped to move the focus into the outline list (Ctrl+Alt+O). */
  outlineFocusTick = $state(0);
  /** Simple mode has no right panel: the outline opens as a popover under the header. */
  outlinePopover = $state(false);
  zoomImage = $state<string | null>(null);
  followChip = $state.raw<{ from: string; to: string; n: number } | null>(null);
  /** The "keep settings with the project" hint was dismissed or used (remembered across projects). */
  hintDismissed = $state(readHintDismissed());
  /** Folders whose "keep settings with the project?" banner was answered "Not now" since the app started. */
  initHintSnoozed = new SvelteSet<string>();
  /** Repo state whose banner the user dismissed; it returns when the state changes. */
  dismissedRepoState = $state<string | null>(null);
  searchFocusTick = $state(0);
  explorerFocusTick = $state(0);
  tagsFocusTick = $state(0);
  /** The tag editor popover; `anchor` is where it opens (null: next to the document header). */
  tagEditor = $state.raw<{ anchor: Anchor | null } | null>(null);
  /**
   * The menu of places the reader came from or can go to again, under the Back or Forward button: `view` says which side
   * ("all" is the whole trail, opened from the command palette), `anchor` is the button (null: the header's buttons).
   */
  trail = $state.raw<{ view: "back" | "forward" | "all"; anchor: Anchor | null } | null>(null);
  /** The folder the name of a new file is being typed in (the input shows in the tree under it). */
  newFile = $state.raw<{ folder: string; n: number } | null>(null);
  /** The tree row the person last moved to or clicked: where Ctrl+N puts a new file. */
  treeCursor = $state.raw<{ path: string; isDir: boolean } | null>(null);
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

  /** Opens the box, or switches it when it is open: at `scope`, else at the scope Ctrl+T remembers. */
  openSearch(scope: Scope = this.searchScope): void {
    this.searchBox = { scope };
  }

  closeSearch(): void {
    this.searchBox = null;
  }

  openTagEditor(anchor: Anchor | null = null): void {
    this.tagEditor = { anchor };
  }

  toggleSection(id: string): void {
    if (this.collapsedSections.has(id)) this.collapsedSections.delete(id);
    else this.collapsedSections.add(id);
  }
}

export const ui = new UiStore();
