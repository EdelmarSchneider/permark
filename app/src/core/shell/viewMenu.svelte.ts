// The options menu of a view, one for the window: where it is open, its entries (move among the groups of the view's
// own region, show as a tab | in a group of its own, close, reset) and what follows a move (the keyboard on the moved
// view, the move said). A tab strip, a view's header (right click | Shift+F10 | the Menu key) and the options button
// in that header all open this one menu; `ViewHosts` draws it.

import { tick } from "svelte";
import type { MenuEntry } from "../explorer/menu";
import { t } from "../i18n";
import {
  canJoin,
  canMove,
  canSplit,
  defaultArrangement,
  joinAsTab,
  moveView,
  splitOut,
  withRegion,
  type Region,
  type ViewGroup,
} from "./arrangement";
import { arrangement } from "./arrangement.svelte";
import { closeView, selectView } from "./sides";

type Move = "up" | "down" | "tab" | "above" | "below";

/** The menu open for the view `id`, at a point of the window, opened from `anchor`. */
type OpenViewMenu = {
  readonly id: string;
  readonly x: number;
  readonly y: number;
  readonly anchor: HTMLElement;
};

class ViewMenu {
  open = $state.raw<OpenViewMenu | null>(null);

  /** There is something to arrange in `region`: two present views or more (else no options, no strip menu). */
  arranging(region: Region | null): boolean {
    return region !== null && arrangement.presentIn(region) >= 2;
  }

  /** Opens the options of `id` at a point (a right click, a key on a tab | header). */
  at(id: string, point: { x: number; y: number }, anchor: HTMLElement): void {
    this.open = { id, x: point.x, y: point.y, anchor };
  }

  /** The options button of `id`: the menu opens under it; pressed again, it closes. */
  toggle(id: string, button: HTMLElement): void {
    if (this.open?.id === id) {
      this.open = null;
      return;
    }
    const box = button.getBoundingClientRect();
    this.at(id, { x: box.left, y: box.bottom + 2 }, button);
  }

  close(): void {
    this.open = null;
  }

  /** After a move: the moved view's options (Show as tab: its tab) take the keyboard; the move is said. */
  async #moved(id: string, what: Move): Promise<void> {
    const name = arrangement.nameOf(id);
    const words = {
      up: t("panel.moved.up", { name }),
      down: t("panel.moved.down", { name }),
      tab: t("panel.moved.tab", { name }),
      above: t("panel.moved.above", { name }),
      below: t("panel.moved.below", { name }),
    } as const;
    arrangement.status = words[what];
    await tick();
    const target =
      what === "tab"
        ? document.querySelector<HTMLElement>(`[role="tab"][data-view="${id}"]`)
        : document.querySelector<HTMLElement>(`[data-view-menu="${id}"]`);
    target?.focus();
  }

  /** The entries for `id`: they act inside the region that lists it. A view nobody lists has none. */
  entries(id: string): MenuEntry[] {
    const region = arrangement.regionOf(id);
    if (region === null) return [];
    const layout = arrangement.layout;
    const mine = layout[region];
    const stacked = arrangement.stacked(region);
    const drawnAt = stacked.map((g) => g.at);
    const short = arrangement.short(region);
    const tabbed = (stacked.find((g) => g.views.includes(id))?.views.length ?? 0) > 1;
    const arrange = (next: readonly ViewGroup[], what: Move): void => {
      arrangement.arrange(withRegion(layout, region, next));
      if (what === "tab") selectView(id);
      void this.#moved(id, what);
    };
    const entries: MenuEntry[] = [];
    if (short) entries.push({ kind: "heading", label: t("panel.tooShort") });
    if (tabbed) {
      entries.push(
        {
          kind: "item",
          label: t("panel.showAbove"),
          disabled: short || !canSplit(mine, id),
          run: () => arrange(splitOut(mine, id, "before"), "above"),
        },
        {
          kind: "item",
          label: t("panel.showBelow"),
          disabled: short || !canSplit(mine, id),
          run: () => arrange(splitOut(mine, id, "after"), "below"),
        },
      );
    } else {
      entries.push(
        {
          kind: "item",
          label: t("panel.moveUp"),
          disabled: short || !canMove(mine, id, -1, drawnAt),
          run: () => arrange(moveView(mine, id, -1, drawnAt), "up"),
        },
        {
          kind: "item",
          label: t("panel.moveDown"),
          disabled: short || !canMove(mine, id, 1, drawnAt),
          run: () => arrange(moveView(mine, id, 1, drawnAt), "down"),
        },
        {
          kind: "item",
          label: t("panel.showAsTab"),
          disabled: short || !canJoin(mine, id, drawnAt),
          run: () => arrange(joinAsTab(mine, id, drawnAt), "tab"),
        },
      );
    }
    // a view that is always open (Files, Search, a plugin's sidebar tab) has no Close
    if (arrangement.viewOf(id)?.closable === true) {
      entries.push(
        { kind: "separator" },
        { kind: "item", label: t("panel.closeView"), run: () => closeView(id) },
      );
    }
    entries.push(
      { kind: "separator" },
      {
        kind: "item",
        label: t("panel.resetLayout"),
        run: () => {
          arrangement.arrange(withRegion(arrangement.saved, region, defaultArrangement()[region]));
          arrangement.status = t("panel.layoutReset");
        },
      },
    );
    return entries;
  }
}

export const viewMenu = new ViewMenu();
