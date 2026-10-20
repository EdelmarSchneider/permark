// The tabs of this view's window, as the top bar chips and the tab keys read them. The backend owns the list
// (`windows-changed` carries every window's); this keeps the last one it sent and acts through it.

import { api, type Api, type TabSummary, type Unlisten, type WindowSummary } from "../../api";
import { canSwitch, last, neighbor, nth, otherWindows } from "./switcherModel";

export type MoveResult = "moved" | "refused" | "busy";

class Tabs {
  windows = $state.raw<readonly WindowSummary[]>([]);
  /** The native window this view lives in. */
  window = $state("");
  #api: Api = api;
  /** Tabs with a move asked and not answered yet (a second click does not ask again). */
  #moving = new Set<number>();

  /** This window's entry. */
  get mine(): WindowSummary | null {
    return this.windows.find((w) => w.window === this.window) ?? null;
  }

  /** This window's tabs, in strip order. */
  get list(): readonly TabSummary[] {
    return this.mine?.tabs ?? [];
  }

  /** This window's active tab. */
  get active(): number | null {
    return this.mine?.active ?? null;
  }

  /** The other windows "Move to window" can take a tab to (not asked to close), focused last first. */
  get others(): readonly WindowSummary[] {
    return otherWindows(this.windows, this.window);
  }

  /** There is another tab to go to (next | previous). */
  get canSwitch(): boolean {
    return canSwitch(this.list);
  }

  /** Listens, then reads the list (nothing is lost in between). Returns the teardown. */
  start(window: string, backend: Api = api): Unlisten {
    this.#api = backend;
    this.window = window;
    // what an earlier start knew is stale
    this.windows = [];
    let gone = false;
    let off: Unlisten | null = null;
    void backend
      .on("windows-changed", (event) => {
        this.windows = event.windows;
      })
      .then(async (stop) => {
        if (gone) {
          stop();
          return;
        }
        off = stop;
        const listed = await backend.listWindows();
        if (!gone && listed.ok) this.windows = listed.value;
      });
    return () => {
      gone = true;
      off?.();
    };
  }

  activate(tab: number): void {
    if (tab !== this.active) void this.#api.tabActivate(tab);
  }

  /** Closes a tab (its view asks first when it holds unsaved edits). */
  close(tab: number): void {
    void this.#api.tabClose(tab);
  }

  /**
   * Moves a tab to a new window. `refused`: it could not move (it stays where it was); `busy`: a move of it is under
   * way already (that one reports), nothing asked.
   */
  async moveToNewWindow(tab: number): Promise<MoveResult> {
    if (this.list.length < 2 && !this.#moving.has(tab)) return "refused";
    return this.#move(tab, () => this.#api.tabMoveNewWindow(tab));
  }

  /** Moves a tab into another window (its only tab too: this window then closes). Results as `moveToNewWindow`. */
  moveToWindow(tab: number, window: string): Promise<MoveResult> {
    if (window === this.window) return Promise.resolve("refused");
    return this.#move(tab, () => this.#api.tabMove(tab, window));
  }

  async #move(tab: number, ask: () => Promise<{ ok: boolean }>): Promise<MoveResult> {
    if (this.#moving.has(tab)) return "busy";
    this.#moving.add(tab);
    try {
      return (await ask()).ok ? "moved" : "refused";
    } finally {
      this.#moving.delete(tab);
    }
  }

  /** The next | previous tab (wraps; nothing with fewer than two). */
  step(step: 1 | -1): void {
    const next = neighbor(this.list, this.active, step);
    if (next !== null) this.activate(next.tab);
  }

  /** Tab `n` (1-based); none -> nothing. */
  select(n: number): void {
    const tab = nth(this.list, n);
    if (tab !== null) this.activate(tab.tab);
  }

  selectLast(): void {
    const tab = last(this.list);
    if (tab !== null) this.activate(tab.tab);
  }
}

export const tabs = new Tabs();
export type { Tabs };
