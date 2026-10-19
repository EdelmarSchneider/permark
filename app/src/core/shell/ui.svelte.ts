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

/** The sections of the settings page. */
export type SettingsSection = "reading" | "editing" | "integration" | "plugins" | "project";
export const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  "reading",
  "editing",
  "integration",
  "plugins",
  "project",
];
export type SettingsPage = { status: "closed" } | { status: "open"; section: SettingsSection };
/** The "Initialize PerMark" question: which folder (`path`, absolute) and what it is called. */
export type InitDialog = { status: "closed" } | { status: "asking"; path: string; name: string };

class UiStore {
  sidebar = $state<SidebarView>("files");
  /** The id of the plugin panel at the right of the page (`null`: none; the outline shows when it is on). */
  right = $state<string | null>(null);
  finder = $state.raw<Finder>({ status: "closed" });
  keyboard = $state(false);
  settings = $state(false);
  settingsPage = $state.raw<SettingsPage>({ status: "closed" });
  initDialog = $state.raw<InitDialog>({ status: "closed" });
  finding = $state(false);
  /** Counts the requests to put the cursor in the Search panel's box. */
  searchFocus = $state(0);
  #seed = 0;

  /** An overlay owns the keyboard. */
  get modal(): boolean {
    return (
      this.finder.status === "open" ||
      this.keyboard ||
      this.settingsPage.status === "open" ||
      this.initDialog.status === "asking"
    );
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

  /** The full settings page (the popover closes). */
  openSettingsPage(section: SettingsSection = "reading"): void {
    this.settings = false;
    this.settingsPage = { status: "open", section };
  }

  showSettingsSection(section: SettingsSection): void {
    if (this.settingsPage.status === "open") this.settingsPage = { status: "open", section };
  }

  closeSettingsPage(): void {
    this.settingsPage = { status: "closed" };
  }

  /** Asks whether to initialize PerMark in a folder. */
  askInit(path: string, name: string): void {
    this.initDialog = { status: "asking", path, name };
  }

  closeInit(): void {
    this.initDialog = { status: "closed" };
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
