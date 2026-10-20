// Back / forward trail of one window: places visited (document + anchor), a cursor, no duplicates in a row (pure).

export type Place = { path: string; anchor: string | null };

const KEPT = 100;
/** Places the trail menu lists. */
export const TRAIL_MENU = 30;

function same(a: Place, b: Place): boolean {
  return a.path === b.path && a.anchor === b.anchor;
}

export class Trail {
  #places: Place[] = [];
  #at = -1;

  get here(): Place | null {
    return this.#places[this.#at] ?? null;
  }

  get canBack(): boolean {
    return this.#at > 0;
  }

  get canForward(): boolean {
    return this.#at >= 0 && this.#at < this.#places.length - 1;
  }

  /** A new place: what was forward is gone; the same place twice in a row counts once. */
  visit(place: Place): void {
    const here = this.here;
    if (here !== null && same(here, place)) return;
    this.#places = [...this.#places.slice(0, this.#at + 1), place].slice(-KEPT);
    this.#at = this.#places.length - 1;
  }

  /** Steps back; the place to show, or null at the start. */
  back(): Place | null {
    if (!this.canBack) return null;
    this.#at -= 1;
    return this.here;
  }

  forward(): Place | null {
    if (!this.canForward) return null;
    this.#at += 1;
    return this.here;
  }

  /** Moves the cursor to a place of the trail (the menu); the place, or null when `index` is not one. */
  jump(index: number): Place | null {
    if (index < 0 || index >= this.#places.length) return null;
    this.#at = index;
    return this.here;
  }

  /** The last places, newest first, with their index for [`jump`] and whether it is where the person is. */
  recent(count = TRAIL_MENU): { index: number; place: Place; current: boolean }[] {
    return this.#places
      .map((place, index) => ({ index, place, current: index === this.#at }))
      .reverse()
      .slice(0, count);
  }

  /** Forgets a document (it was deleted | the workspace changed). */
  clear(): void {
    this.#places = [];
    this.#at = -1;
  }
}
