// Window UI state the commands act on: which sidebar view shows, the Search everywhere box, the keyboard dialog,
// settings, find in page. Small tagged states; components read, commands call the methods.

import type { ClosingTab } from "../../api";
import type { Overlay } from "../commands/dispatch";
import type { Preset } from "../search/finderModel";
import { layout } from "./layout.svelte";

/** `files`, `search`, or the id of a panel a plugin put in the sidebar. */
export type SidebarView = string;
/** The three faces of the one overlay: Quick open, Search everywhere, the command palette. */
export type FinderPreset = Preset;
export type Finder =
  | { status: "closed" }
  /** `seed` changes on every open: the box starts fresh. */
  | { status: "open"; preset: FinderPreset; seed: number };

/** The sections of the settings page. */
export type SettingsSection =
  "reading" | "editing" | "integration" | "plugins" | "projects" | "project";
export const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  "reading",
  "editing",
  "integration",
  "plugins",
  "projects",
  "project",
];
export type SettingsPage = { status: "closed" } | { status: "open"; section: SettingsSection };
/** The "Initialize PerMark" question: which folder (`path`, absolute), what it is called, and whether it opens once
 *  set up (New project; otherwise it is the project shown already). */
export type InitDialog =
  { status: "closed" } | { status: "asking"; path: string; name: string; opens: boolean };

/** Focus mode: only the page. */
export type Focus = "off" | "on";
/** How long the frame's tracks animate after a toggle (focus mode, a side panel shown | hidden) (ms): a little over the
 *  180 ms transition. */
export const FOCUS_MOVE_MS = 260;

class UiStore {
  sidebar = $state<SidebarView>("files");
  /** Focus mode of this window (not stored: a new window or a restart starts `off`). */
  focus = $state<Focus>("off");
  /** The 260 ms after a toggle (focus mode, a side panel): the frame's tracks transition only then (dragging a splitter
   *  never animates). */
  frameMoving = $state(false);
  #moving: ReturnType<typeof setTimeout> | null = null;
  /** The id of the plugin panel at the right of the page (`null`: none; the outline shows when it is on). */
  right = $state<string | null>(null);
  finder = $state.raw<Finder>({ status: "closed" });
  keyboard = $state(false);
  settings = $state(false);
  settingsPage = $state.raw<SettingsPage>({ status: "closed" });
  initDialog = $state.raw<InitDialog>({ status: "closed" });
  finding = $state(false);
  /** The window close prompt (this view is the one on screen): the tabs with unsaved edits; `null`: not asking. */
  closePrompt = $state.raw<readonly ClosingTab[] | null>(null);
  /** Counts the requests to put the cursor in the Search panel's box. */
  searchFocus = $state(0);
  #seed = 0;

  /** What owns the keyboard: the close prompt (a question: no command runs), a box, nothing. */
  get overlay(): Overlay {
    if (this.closePrompt !== null) return "question";
    const box =
      this.finder.status === "open" ||
      this.keyboard ||
      this.settingsPage.status === "open" ||
      this.initDialog.status === "asking";
    return box ? "box" : "none";
  }

  /** An overlay owns the keyboard. */
  get modal(): boolean {
    return this.overlay !== "none";
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

  /** Shows the plugin panel `id` at the right, or closes it when it is the one that shows. The right panel hidden: it
   *  shows again, with that panel. */
  toggleRight(id: string): void {
    if (layout.rightHidden) {
      this.animateFrame();
      layout.setRightHidden(false);
      this.right = id;
      return;
    }
    this.right = this.right === id ? null : id;
  }

  /** The frame's tracks transition for a moment (a toggle); never while a splitter is dragged. */
  animateFrame(): void {
    this.frameMoving = true;
    if (this.#moving !== null) clearTimeout(this.#moving);
    this.#moving = setTimeout(() => {
      this.#moving = null;
      this.frameMoving = false;
    }, FOCUS_MOVE_MS);
  }

  /** Enters or leaves focus mode (the frame animates unless the person asked for less motion). */
  setFocus(next: Focus): void {
    if (next === this.focus) return;
    this.focus = next;
    this.animateFrame();
  }

  toggleFocus(): void {
    this.setFocus(this.focus === "on" ? "off" : "on");
  }

  /** The document area went away (another workspace, the Welcome): focus ends at once, no animation. */
  resetFocus(): void {
    if (this.#moving !== null) clearTimeout(this.#moving);
    this.#moving = null;
    this.frameMoving = false;
    this.focus = "off";
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
  askInit(path: string, name: string, opens = false): void {
    this.initDialog = { status: "asking", path, name, opens };
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
