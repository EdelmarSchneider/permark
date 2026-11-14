// The symbols panel's state for this window: the search and the groups the person collapsed (kept while the view
// moves between regions; not stored: a new window starts afresh), and the group to bring into view once, when the
// emoji popup handed over to the panel. The panel is offered (a view of the arrangement, a thing the right region's
// toggle can open) only once the person asked for it in this window, or while it is open for the project: a window
// that never used it shows nothing new.

import { groupsFor, type PickerKind } from "../../core/editing/visual/characters";
import { layout } from "../../core/shell/layout.svelte";
import { showView, togglePanel } from "../../core/shell/sides";
import { windowStore } from "../../core/window.svelte";
import { DEFAULT_COLLAPSED } from "./model";

/** The id of the symbols panel. */
export const PANEL = "symbols.panel";

export class SymbolsStore {
  query = $state("");
  /** Section keys of the groups drawn collapsed. */
  collapsed = $state.raw<ReadonlySet<string>>(DEFAULT_COLLAPSED);
  /** A group the panel scrolls to once it draws it (`null`: none asked). */
  reveal = $state<string | null>(null);
  /** The person asked for the panel in this window (the command, the popup's button). */
  called = $state(false);

  /** The panel is a view of this window now (reactive). */
  get offered(): boolean {
    return this.called || layout.views.open[PANEL] === true;
  }

  /** Expands | collapses one group. */
  setExpanded(key: string, expanded: boolean): void {
    if (expanded !== this.collapsed.has(key)) return;
    const next = new Set(this.collapsed);
    if (expanded) next.delete(key);
    else next.add(key);
    this.collapsed = next;
  }

  /** The palette command: the panel shows | closes. */
  toggle(): void {
    this.called = true;
    togglePanel(PANEL);
  }

  /** The popup's "Keep open in a panel": the panel shows (the keyboard stays in the editor); from the emoji popup
   *  its group opens and comes into view. */
  keepOpen(picker: PickerKind): void {
    this.called = true;
    if (picker === "emoji") {
      const emoji = groupsFor("emoji", windowStore.characters)[0]?.name;
      if (emoji !== undefined) {
        this.setExpanded(emoji, true);
        this.reveal = emoji;
      }
    }
    showView(PANEL);
  }

  /** The plugin is switched off: nothing of this window's panel state is left. */
  reset(): void {
    this.query = "";
    this.collapsed = DEFAULT_COLLAPSED;
    this.reveal = null;
    this.called = false;
  }
}

export const symbols = new SymbolsStore();
