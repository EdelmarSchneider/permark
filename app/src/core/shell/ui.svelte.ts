// Window UI state the commands act on: which sidebar view shows, the Search everywhere box, the keyboard dialog,
// settings, find in page. Small tagged states; components read, commands call the methods.

import type { Preset } from "../search/finderModel";

/** `files`, `search`, or the id of a panel a plugin put in the sidebar. */
export type SidebarView = string;
/** The three faces of the one overlay: Quick open, Search everywhere, the command palette. */
export type FinderPreset = Preset;
export type Finder =
  | { status: "closed" }
  /** `seed` changes on every open: the box starts fresh. */
  | { status: "open"; preset: FinderPreset; seed: number };

class UiStore {
  sidebar = $state<SidebarView>("files");
  /** The id of the plugin panel at the right of the page (`null`: none; the outline shows when it is on). */
  right = $state<string | null>(null);
  finder = $state.raw<Finder>({ status: "closed" });
  keyboard = $state(false);
  settings = $state(false);
  finding = $state(false);
  /** Counts the requests to put the cursor in the Search panel's box. */
  searchFocus = $state(0);
  #seed = 0;

  /** An overlay owns the keyboard. */
  get modal(): boolean {
    return this.finder.status === "open" || this.keyboard;
  }

  openFinder(preset: FinderPreset): void {
    this.#seed += 1;
    this.finder = { status: "open", preset, seed: this.#seed };
  }

  closeFinder(): void {
    this.finder = { status: "closed" };
  }

  showSidebar(view: SidebarView): void {
    this.sidebar = view;
    if (view === "search") this.searchFocus += 1;
  }

  /** Shows the plugin panel `id` at the right, or closes it when it is the one that shows. */
  toggleRight(id: string): void {
    this.right = this.right === id ? null : id;
  }

  openSettings(): void {
    this.settings = true;
  }

  toggleSettings(): void {
    this.settings = !this.settings;
  }

  closeSettingsPopover(): void {
    this.settings = false;
  }

  openKeyboard(): void {
    this.settings = false;
    this.keyboard = true;
  }

  closeKeyboard(): void {
    this.keyboard = false;
  }

  openFind(): void {
    this.finding = true;
  }

  closeFind(): void {
    this.finding = false;
  }
}

export const ui = new UiStore();
export type { UiStore };
