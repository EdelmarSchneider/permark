// Widths of the two side panels of this window. A project window keeps them in the project session (written a moment
// after the last change); a single file | the Notes keep them for the life of the window.

import type { Api } from "../../api";
import { clampWidth, RIGHT, SIDEBAR } from "./layout";

const KEYS = { sidebar: "layout.sidebar", right: "layout.right" } as const;
const SAVE_DELAY_MS = 400;

export class LayoutStore {
  sidebar = $state(SIDEBAR.def);
  right = $state(RIGHT.def);
  #api: Api | null = null;
  #timer: ReturnType<typeof setTimeout> | null = null;
  /** A project window writes its widths; others only keep them. */
  #persist = false;

  /** A workspace is shown: its own widths (a project's session), else the defaults. */
  async load(api: Api, project: boolean): Promise<void> {
    this.#api = api;
    this.#persist = project;
    this.#cancel();
    this.sidebar = SIDEBAR.def;
    this.right = RIGHT.def;
    if (!project) return;
    const [sidebar, right] = await Promise.all([
      api.projectSessionGet(KEYS.sidebar),
      api.projectSessionGet(KEYS.right),
    ]);
    if (api !== this.#api) return;
    if (sidebar.ok && typeof sidebar.value === "number") {
      this.sidebar = clampWidth(SIDEBAR, sidebar.value);
    }
    if (right.ok && typeof right.value === "number") this.right = clampWidth(RIGHT, right.value);
  }

  setSidebar(px: number): void {
    this.sidebar = clampWidth(SIDEBAR, px);
    this.#save();
  }

  setRight(px: number): void {
    this.right = clampWidth(RIGHT, px);
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
      void this.#api?.projectSessionSet(KEYS.sidebar, this.sidebar);
      void this.#api?.projectSessionSet(KEYS.right, this.right);
    }, SAVE_DELAY_MS);
  }
}

export const layout = new LayoutStore();
