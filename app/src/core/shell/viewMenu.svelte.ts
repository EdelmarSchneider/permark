// The options menu of a view, one for the window: where it is open, its entries (the moves among the groups of the
// view's own region, worded by the direction that region stacks its groups; the moves to another region; close; the
// saved layouts and the reset of the whole arrangement) and what follows an action (the keyboard on the view's options, the move said).
// A tab strip (its options button; a right click | Shift+F10 | the Menu key on a tab) and a view's header (its
// options button; the same click and keys) all open this one menu; `ViewHosts` draws it.

import { tick } from "svelte";
import type { MenuEntry } from "../explorer/menu";
import { t, type MessageKey } from "../i18n";
import {
  axisOf,
  canJoin,
  canMove,
  canSplit,
  inRegion,
  joinAsTab,
  moveView,
  REGIONS,
  splitOut,
  type Region,
  type ViewGroup,
} from "./arrangement";
import { arrangement } from "./arrangement.svelte";
import { ACROSS, ALONG, AS_TAB, TO_BOTTOM_UNSEEN, type Words } from "./moveWords";
import { bottomSqueezed, closeView, focusView, moveToRegion, selectView } from "./sides";
import { ui } from "./ui.svelte";

/** The menu open for the view `id`, at a point of the window, opened from `anchor`. */
type OpenViewMenu = {
  readonly id: string;
  readonly x: number;
  readonly y: number;
  readonly anchor: HTMLElement;
};

class ViewMenu {
  open = $state.raw<OpenViewMenu | null>(null);

  /**
   * The options buttons are drawn: the window has two present views or more (one view alone, a single file with its
   * outline, keeps the screen it always had; its menu still opens by a right click | Shift+F10 | the Menu key).
   */
  get hasButton(): boolean {
    return arrangement.presentCount >= 2;
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

  /** Once what an action changed is drawn, the keyboard goes to the view the menu was for (`focusView`). */
  async #focus(id: string): Promise<void> {
    await tick();
    focusView(id);
  }

  /**
   * After a move (an action of the menu, a view dropped): it is said, and the moved view's options take the
   * keyboard. Into a bottom region the window is too short to draw: the move is made all the same, so the words
   * add how to see the view.
   */
  moved(id: string, said: MessageKey): void {
    const unseen = said === ACROSS.bottom.said && bottomSqueezed();
    arrangement.say(t(unseen ? TO_BOTTOM_UNSEEN : said, { name: arrangement.nameOf(id) }));
    void this.#focus(id);
  }

  /**
   * The moves among the groups of the view's own region: alone in its group, one group back | forth or into the
   * group beside it as a tab; a tab of a group, out into a group of its own. None while the region has one present
   * view; all there but unable to act, under a line that says why, while a small window folds the region's groups.
   */
  #within(id: string, region: Region): MenuEntry[] {
    if (arrangement.presentIn(region) < 2) return [];
    const layout = arrangement.layout;
    const mine = layout[region];
    const stacked = arrangement.stacked(region);
    const drawnAt = stacked.map((g) => g.at);
    const folded = arrangement.short(region);
    const words = ALONG[axisOf(region)];
    const tabbed = (stacked.find((g) => g.views.includes(id))?.views.length ?? 0) > 1;
    const item = (
      move: Words,
      can: boolean,
      op: (groups: readonly ViewGroup[]) => readonly ViewGroup[],
    ): MenuEntry => ({
      kind: "item",
      label: t(move.label),
      disabled: folded || !can,
      run: () => {
        arrangement.arrange(inRegion(layout, id, op));
        // the moved view is the one its group shows: its options are there to take the keyboard
        if (!arrangement.isSelected(id)) selectView(id);
        this.moved(id, move.said);
      },
    });
    const moves = tabbed
      ? [
          item(words.before, canSplit(mine, id), (groups) => splitOut(groups, id, "before")),
          item(words.after, canSplit(mine, id), (groups) => splitOut(groups, id, "after")),
        ]
      : [
          item(words.back, canMove(mine, id, -1, drawnAt), (groups) =>
            moveView(groups, id, -1, drawnAt),
          ),
          item(words.forth, canMove(mine, id, 1, drawnAt), (groups) =>
            moveView(groups, id, 1, drawnAt),
          ),
          item(AS_TAB, canJoin(mine, id, drawnAt), (groups) => joinAsTab(groups, id, drawnAt)),
        ];
    return folded ? [{ kind: "heading", label: t(words.folded) }, ...moves] : moves;
  }

  /**
   * The entries for `id`, in blocks: the moves inside the region that lists it, the moves to the other regions,
   * Close (a view the person can close), the whole arrangement (the Layouts dialog, the reset). A view nobody lists
   * has none.
   */
  entries(id: string): MenuEntry[] {
    const region = arrangement.regionOf(id);
    if (region === null) return [];
    const blocks: MenuEntry[][] = [
      this.#within(id, region),
      REGIONS.filter((other) => other !== region).map((other) => ({
        kind: "item",
        label: t(ACROSS[other].label),
        run: () => {
          moveToRegion(id, other);
          this.moved(id, ACROSS[other].said);
        },
      })),
      // a view that is always open (Files, Search, a plugin's sidebar tab) has no Close
      arrangement.viewOf(id)?.closable === true
        ? [
            {
              kind: "item",
              label: t("panel.closeView"),
              run: () => {
                closeView(id, true);
                arrangement.say(t("panel.closed", { name: arrangement.nameOf(id) }));
              },
            },
          ]
        : [],
      [
        { kind: "item", label: t("layouts.open"), run: () => ui.openLayouts() },
        {
          kind: "item",
          label: t("panel.resetLayout"),
          disabled: arrangement.atDefault,
          run: () => {
            arrangement.reset();
            void this.#focus(id);
          },
        },
      ],
    ];
    return blocks
      .filter((block) => block.length > 0)
      .flatMap((block, at): MenuEntry[] => (at === 0 ? block : [{ kind: "separator" }, ...block]));
  }
}

export const viewMenu = new ViewMenu();
