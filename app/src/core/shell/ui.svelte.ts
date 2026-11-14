// Window UI state the commands act on: focus mode, the Search everywhere box, the keyboard dialog, the Layouts dialog,
// settings, find in page. Small tagged states; components read, commands call the methods. Which view a region shows is not here: the
// arrangement of the views has it (`arrangement.svelte.ts`, `sides.ts`).

import type { ClosingTab } from "../../api";
import type { Overlay } from "../commands/dispatch";
import type { Preset } from "../search/finderModel";

/** The three faces of the one overlay: Quick open, Search everywhere, the command palette. */
export type FinderPreset = Preset;
export type Finder =
  | { status: "closed" }
  /** `seed` changes on every open: the box starts fresh. */
  | { status: "open"; preset: FinderPreset; seed: number };

/** The core sections of the settings page. */
export type CoreSettingsSection =
  "reading" | "editing" | "integration" | "plugins" | "projects" | "project";
export const SETTINGS_SECTIONS: readonly CoreSettingsSection[] = [
  "reading",
  "editing",
  "integration",
  "plugins",
  "projects",
  "project",
];
/** Where each core section sits among the plugins' own (`settingsPage { order }`). */
export const CORE_SECTION_ORDER: Readonly<Record<CoreSettingsSection, number>> = {
  reading: 10,
  editing: 20,
  integration: 40,
  plugins: 50,
  projects: 60,
  project: 70,
};
/** A plugin's own section (`<plugin>.<name>`: the dot keeps it apart from every core section name). */
export type PluginPageId = `${string}.${string}`;
/** A section of the settings page: a core one or a plugin's own. */
export type SettingsSection = CoreSettingsSection | PluginPageId;
/** Where focus goes when the page opens: the nav (the section list) or the section's first control. */
export type SettingsEntry = "nav" | "content";
export type SettingsPage = { status: "closed" } | { status: "open"; section: SettingsSection };
/** The "Initialize PerMark" question: which folder (`path`, absolute), what it is called, and whether it opens once
 *  set up (New project; otherwise it is the project shown already). */
export type InitDialog =
  { status: "closed" } | { status: "asking"; path: string; name: string; opens: boolean };
/** The Layouts dialog: the arrangements of the views saved under a name. */
export type LayoutsDialog = { status: "closed" } | { status: "open" };

/** Focus mode: only the page. */
export type Focus = "off" | "on";
/** How long the frame's tracks animate after a toggle (focus mode, a side panel shown | hidden) (ms): a little over the
 *  180 ms transition. */
export const FOCUS_MOVE_MS = 260;

class UiStore {
  /** Focus mode of this window (not stored: a new window or a restart starts `off`). */
  focus = $state<Focus>("off");
  /** The 260 ms after a toggle (focus mode, a side panel): the frame's tracks transition only then (dragging a splitter
   *  never animates). */
  frameMoving = $state(false);
  #moving: ReturnType<typeof setTimeout> | null = null;
  finder = $state.raw<Finder>({ status: "closed" });
  keyboard = $state(false);
  settings = $state(false);
  settingsPage = $state.raw<SettingsPage>({ status: "closed" });
  /** The section asked to take focus when it shows (`takeContentFocus`). Not reactive: taking it changes nothing on
   *  screen (a write to the page state would draw the section again). */
  #contentFocus: SettingsSection | null = null;
  initDialog = $state.raw<InitDialog>({ status: "closed" });
  layoutsDialog = $state.raw<LayoutsDialog>({ status: "closed" });
  finding = $state(false);
  /** The window close prompt (this view is the one on screen): the tabs with unsaved edits; `null`: not asking. */
  closePrompt = $state.raw<readonly ClosingTab[] | null>(null);
  /** Counts the requests to put the cursor in the Search panel's box (the Search view brought forward). */
  searchFocus = $state(0);
  #seed = 0;

  /** What owns the keyboard: the close prompt (a question: no command runs), a box, nothing. */
  get overlay(): Overlay {
    if (this.closePrompt !== null) return "question";
    const box =
      this.finder.status === "open" ||
      this.keyboard ||
      this.settingsPage.status === "open" ||
      this.initDialog.status === "asking" ||
      this.layoutsDialog.status === "open";
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

  /** The full settings page (the popover closes). `entry` `content`: the section focuses its first control. */
  openSettingsPage(section: SettingsSection = "reading", entry: SettingsEntry = "nav"): void {
    this.settings = false;
    this.settingsPage = { status: "open", section };
    this.#contentFocus = entry === "content" ? section : null;
  }

  showSettingsSection(section: SettingsSection): void {
    if (this.settingsPage.status !== "open") return;
    this.settingsPage = { status: "open", section };
    this.#contentFocus = null;
  }

  /**
   * Whether the open section `section` was asked to take focus (opened with `entry` `content`); asking answers once
   * (the request is spent: showing the section again from the nav keeps focus there).
   */
  takeContentFocus(section: SettingsSection): boolean {
    const page = this.settingsPage;
    if (page.status !== "open" || page.section !== section || this.#contentFocus !== section) {
      return false;
    }
    this.#contentFocus = null;
    return true;
  }

  closeSettingsPage(): void {
    this.settingsPage = { status: "closed" };
    this.#contentFocus = null;
  }

  /** Asks whether to initialize PerMark in a folder. */
  askInit(path: string, name: string, opens = false): void {
    this.initDialog = { status: "asking", path, name, opens };
  }

  closeInit(): void {
    this.initDialog = { status: "closed" };
  }

  openLayouts(): void {
    this.settings = false;
    this.layoutsDialog = { status: "open" };
  }

  closeLayouts(): void {
    this.layoutsDialog = { status: "closed" };
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
