// The tabs of this view's window, as the top bar chips and the tab keys read them. The backend owns the list
// (`windows-changed` carries every window's); this keeps the last one it sent and acts through it.

import { api, type Api, type TabSummary, type Unlisten, type WindowSummary } from "../../api";
import { canSwitch, last, neighbor, nth } from "./switcherModel";

class Tabs {
  windows = $state.raw<readonly WindowSummary[]>([]);
  /** The native window this view lives in. */
  window = $state("");
  #api: Api = api;

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

  moveToNewWindow(tab: number): void {
    if (this.list.length > 1) void this.#api.tabMoveNewWindow(tab);
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
