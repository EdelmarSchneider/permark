// Widths of the two side panels of this window and whether each is hidden. A project window keeps them in the project
// session (written a moment after the last change); a single file | the Notes keep them for the life of the window.

import type { Api } from "../../api";
import { clampWidth, RIGHT, SIDEBAR } from "./layout";
import { emptyProjectViews, parseProjectViews, type ProjectViews } from "./rightViews";

const KEYS = {
  sidebar: "layout.sidebar",
  right: "layout.right",
  sidebarHidden: "layout.sidebar-hidden",
  rightHidden: "layout.right-hidden",
  views: "layout.views",
} as const;
const SAVE_DELAY_MS = 400;

export class LayoutStore {
  sidebar = $state(SIDEBAR.def);
  right = $state(RIGHT.def);
  /** The person hid the sidebar (absent in a session = shown: sessions of 0.6.0 | 0.6.1 open with it shown). */
  sidebarHidden = $state(false);
  /** The person hid the right panel. */
  rightHidden = $state(false);
  /** A project's session is being read: the panels stay off until it says (no flash of a panel it hides). */
  reading = $state(false);
  /** The views of the right column this project opened | closed and the tab each group shows (`rightViews.ts`). */
  views = $state.raw<ProjectViews>(emptyProjectViews());
  /** Counts the project sessions read (each project shown = one more): the views' defaults are decided per load. */
  loads = $state(0);
  #api: Api | null = null;
  #seq = 0;
  #timer: ReturnType<typeof setTimeout> | null = null;
  /** A project window writes its layout; others only keep it. */
  #persist = false;

  /** A workspace is shown: its own layout (a project's session), else the defaults. */
  async load(api: Api, project: boolean): Promise<void> {
    this.#api = api;
    this.#persist = project;
    this.#cancel();
    this.sidebar = SIDEBAR.def;
    this.right = RIGHT.def;
    this.sidebarHidden = false;
    this.rightHidden = false;
    this.views = emptyProjectViews();
    const seq = ++this.#seq;
    this.reading = project;
    if (!project) return;
    const [sidebar, right, sidebarHidden, rightHidden, views] = await Promise.all([
      api.projectSessionGet(KEYS.sidebar),
      api.projectSessionGet(KEYS.right),
      api.projectSessionGet(KEYS.sidebarHidden),
      api.projectSessionGet(KEYS.rightHidden),
      api.projectSessionGet(KEYS.views),
    ]);
    if (seq !== this.#seq) return;
    this.reading = false;
    // damaged | another version | absent (0.6.x sessions) -> nothing chosen yet: each view's default
    this.views = views.ok ? parseProjectViews(views.value) : emptyProjectViews();
    this.loads++;
    if (sidebar.ok && typeof sidebar.value === "number") {
      this.sidebar = clampWidth(SIDEBAR, sidebar.value);
    }
    if (right.ok && typeof right.value === "number") this.right = clampWidth(RIGHT, right.value);
    // only a boolean counts: anything else (a damaged file, a value of a newer app) = shown
    if (sidebarHidden.ok && sidebarHidden.value === true) this.sidebarHidden = true;
    if (rightHidden.ok && rightHidden.value === true) this.rightHidden = true;
  }

  /** The sidebar is off screen: hidden by the person, or its project's session not read yet. */
  get sidebarOff(): boolean {
    return this.sidebarHidden || this.reading;
  }

  get rightOff(): boolean {
    return this.rightHidden || this.reading;
  }

  setSidebar(px: number): void {
    this.sidebar = clampWidth(SIDEBAR, px);
    this.#save();
  }

  setRight(px: number): void {
    this.right = clampWidth(RIGHT, px);
    this.#save();
  }

  setSidebarHidden(hidden: boolean): void {
    if (hidden === this.sidebarHidden) return;
    this.sidebarHidden = hidden;
    this.#save();
  }

  setRightHidden(hidden: boolean): void {
    if (hidden === this.rightHidden) return;
    this.rightHidden = hidden;
    this.#save();
  }

  /** The person opened | closed a view of the right column (remembered for this project). */
  setOpen(id: string, open: boolean): void {
    if (this.views.open[id] === open) return;
    this.views = { ...this.views, open: { ...this.views.open, [id]: open } };
    this.#save();
  }

  /** The tab each group of the right column shows. */
  setSelected(selected: readonly string[]): void {
    const now = this.views.selected;
    if (now.length === selected.length && now.every((id, i) => id === selected[i])) return;
    this.views = { ...this.views, selected: [...selected] };
    this.#save();
  }

  #cancel(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  #save(): void {
    if (!this.#persist) return;
    this.#cancel();
    this.#timer = setTimeout(() => {
      this.#timer = null;
      const api = this.#api;
      if (api === null) return;
      void api.projectSessionSet(KEYS.sidebar, this.sidebar);
      void api.projectSessionSet(KEYS.right, this.right);
      void api.projectSessionSet(KEYS.sidebarHidden, this.sidebarHidden);
      void api.projectSessionSet(KEYS.rightHidden, this.rightHidden);
      void api.projectSessionSet(KEYS.views, {
        v: 1,
        open: { ...this.views.open },
        selected: [...this.views.selected],
      });
    }, SAVE_DELAY_MS);
  }
}

export const layout = new LayoutStore();
