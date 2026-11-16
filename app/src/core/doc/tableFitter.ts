// Column widths of the tables on screen (reading view and Edit), from their measured content (`tableColumns.ts`).
// A table is measured once it comes near the screen (two layouts: at max-content and at min-content width). A resize
// shares the measured columns again without measuring, once the size settles; a font change (zoom, look, a font that
// finished loading) measures again. An edited table is measured again once editing pauses and the caret is out of it,
// so widths never jump while typing. Without the observers (no layout to watch) tables keep the browser's layout.

import { TABLE_WRAP } from "./enhance";
import {
  FIT_TABLE,
  NATURAL_TABLE,
  fitColumns,
  measureColumns,
  screenLayout,
  type ColumnSpan,
  type ScreenLimits,
  type TableLayout,
} from "./tableColumns";

/** Quiet time after the last resize before the columns are shared again. */
export const SETTLE_MS = 100;
/** Quiet time after the last edit of a table before it is measured again (checked again while the caret is in it). */
export const IDLE_MS = 1000;
/** A column whose longest line is at most this wide never wraps: about 12 digits (numbers, dates, ids). */
const SHORT_EM = 7;
/** The widest long text column of a table that scrolls anyway: about 40 characters of prose, a readable line. */
const CAP_EM = 22;

export type Measured = { columns: ColumnSpan[]; limits: ScreenLimits; font: string };

/** What the fitter reads from the page (layout reads: jsdom has none, tests give their own). */
export type TableProbe = {
  /** The table's columns, measured now (its fitted widths are already removed); null: leave it to the browser. */
  measure(table: HTMLTableElement): Measured | null;
  /** The table's font, as one string: measured spans are stale once it changes. */
  font(table: HTMLTableElement): string;
  /** The width the table may take: its wrapper's content box at full width. */
  room(table: HTMLTableElement): number;
};

const px = (value: string): number => Number.parseFloat(value) || 0;

function fontOf(table: HTMLTableElement): string {
  const style = getComputedStyle(table);
  return `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
}

export const screenProbe: TableProbe = {
  measure(table) {
    const columns = measureColumns(table);
    const cell = table.rows[0]?.cells[0];
    if (!columns || !cell) return null;
    const em = px(getComputedStyle(table).fontSize);
    const box = getComputedStyle(cell);
    // the spans are border boxes plus a pixel (`measureColumns`): the limits count the same
    const edge =
      px(box.paddingLeft) +
      px(box.paddingRight) +
      px(box.borderLeftWidth) +
      px(box.borderRightWidth) +
      1;
    const limits = { short: SHORT_EM * em + edge, cap: CAP_EM * em + edge };
    return { columns, limits, font: fontOf(table) };
  },
  font: fontOf,
  room(table) {
    const wrap = table.parentElement;
    const holder = wrap?.parentElement;
    if (!wrap || !holder) return 0;
    // read from the holder: a natural table's wrapper is only as wide as the table
    const inner = getComputedStyle(holder);
    const frame = getComputedStyle(wrap);
    return (
      holder.clientWidth -
      px(inner.paddingLeft) -
      px(inner.paddingRight) -
      px(frame.borderLeftWidth) -
      px(frame.borderRightWidth)
    );
  },
};

type Entry = {
  measured: Measured | null;
  /** The layout shown now, as a key (no DOM write when it would not change). */
  shown: string;
  /** Measured again once editing pauses. */
  dirty: boolean;
  /** Came near the screen while hidden (no room): measured once it is shown. */
  stale: boolean;
  /** Watched for size: the wrapper and the element that holds it. */
  wrap: Element | null;
  holder: Element | null;
};

const AUTO = "auto";

function keyOf(layout: TableLayout): string {
  if (layout.kind === "auto" || layout.kind === "natural") return layout.kind;
  return `${layout.kind}:${layout.widths.map((width) => width.toFixed(1)).join(",")}`;
}

/** The table back to the browser's layout (only what the fitter set). */
function unfit(table: HTMLTableElement): void {
  table.parentElement?.classList.remove(NATURAL_TABLE);
  if (!table.classList.contains(FIT_TABLE)) return;
  table.querySelector(":scope > colgroup")?.remove();
  table.classList.remove(FIT_TABLE);
  table.style.removeProperty("width");
}

function show(table: HTMLTableElement, layout: TableLayout): void {
  unfit(table);
  switch (layout.kind) {
    case "auto":
      break;
    case "natural":
      table.parentElement?.classList.add(NATURAL_TABLE);
      break;
    case "fill":
      fitColumns(table, layout.widths);
      break;
    case "scroll": {
      fitColumns(table, layout.widths);
      const width = layout.widths.reduce((sum, value) => sum + value, 0);
      table.style.width = `${Math.ceil(width)}px`;
      break;
    }
    default:
      layout satisfies never;
  }
}

/** True while the caret is in `table` and its editor has the focus. */
function busy(table: HTMLTableElement): boolean {
  const owner = table.ownerDocument;
  const anchor = owner.getSelection()?.anchorNode ?? null;
  return anchor !== null && table.contains(anchor) && owner.activeElement?.contains(table) === true;
}

export type FitterOptions = {
  probe?: TableProbe;
  /** The pane that scrolls the tables: a table is measured within one pane height of its view. None: the window. */
  root?: () => Element | null;
};

export class TableFitter {
  readonly #probe: TableProbe;
  readonly #root: () => Element | null;
  readonly #entries = new Map<HTMLTableElement, Entry>();
  /** Holders watched for size, with the number of tables in each. */
  readonly #holders = new Map<Element, number>();
  #view: IntersectionObserver | null = null;
  #size: ResizeObserver | null = null;
  #settle: ReturnType<typeof setTimeout> | null = null;
  #idle: ReturnType<typeof setTimeout> | null = null;
  readonly #fontsLoaded = (): void => this.#measure([...this.#entries.keys()]);

  constructor(options: FitterOptions = {}) {
    this.#probe = options.probe ?? screenProbe;
    this.#root = options.root ?? (() => null);
  }

  /** Lays out `table` from its content once it comes near the screen. Returns what forgets it. */
  watch(table: HTMLTableElement): () => void {
    const forget = (): void => this.#forget(table);
    // the document's own column widths (an HTML table) stay as written
    const own =
      !table.classList.contains(FIT_TABLE) &&
      (table.querySelector(":scope > colgroup") !== null || table.style.width !== "");
    if (own || this.#entries.has(table) || !this.#start() || !this.#view) return forget;
    this.#entries.set(table, {
      measured: null,
      shown: AUTO,
      dirty: false,
      stale: false,
      wrap: null,
      holder: null,
    });
    this.#view.observe(table);
    return forget;
  }

  /** Watches every table in a wrapper at or under `root`. Returns what forgets them. */
  watchIn(root: Element): () => void {
    const wraps = [
      ...(root.classList.contains(TABLE_WRAP) ? [root] : []),
      ...root.querySelectorAll(`.${TABLE_WRAP}`),
    ];
    const forgets = wraps
      .map((wrap) => wrap.firstElementChild)
      .filter((table) => table instanceof HTMLTableElement)
      .map((table) => this.watch(table));
    return () => {
      for (const forget of forgets) forget();
    };
  }

  /** The content of `table` changed (an edit): measured again once editing pauses and the caret left it. */
  changed(table: HTMLTableElement): void {
    const entry = this.#entries.get(table);
    if (!entry) return;
    entry.dirty = true;
    this.#armIdle();
  }

  /** Forgets every table and stops watching. */
  stop(): void {
    for (const table of [...this.#entries.keys()]) this.#forget(table);
    this.#halt();
  }

  /** The observers exist (made on the first table); false where the page cannot be watched. */
  #start(): boolean {
    if (this.#view && this.#size) return true;
    if (typeof IntersectionObserver === "undefined" || typeof ResizeObserver === "undefined") {
      return false;
    }
    this.#view = new IntersectionObserver(
      (entries) => {
        const near: HTMLTableElement[] = [];
        for (const entry of entries) {
          if (!entry.isIntersecting || !(entry.target instanceof HTMLTableElement)) continue;
          this.#view?.unobserve(entry.target);
          near.push(entry.target);
        }
        this.#measure(near);
      },
      // one pane height above and below counts as near (as for the blocks' other deferred work); the margin
      // grows the root only, so the root must be the pane itself (the window's margin stops at the pane's edge)
      { root: this.#root(), rootMargin: "100% 0px 100% 0px" },
    );
    this.#size = new ResizeObserver(() => this.#soon());
    if ("fonts" in document) document.fonts.addEventListener("loadingdone", this.#fontsLoaded);
    return true;
  }

  #halt(): void {
    this.#view?.disconnect();
    this.#size?.disconnect();
    this.#view = null;
    this.#size = null;
    this.#holders.clear();
    if (this.#settle !== null) clearTimeout(this.#settle);
    if (this.#idle !== null) clearTimeout(this.#idle);
    this.#settle = null;
    this.#idle = null;
    if ("fonts" in document) document.fonts.removeEventListener("loadingdone", this.#fontsLoaded);
  }

  #forget(table: HTMLTableElement): void {
    const entry = this.#entries.get(table);
    if (!entry) return;
    this.#entries.delete(table);
    this.#view?.unobserve(table);
    if (entry.wrap) this.#size?.unobserve(entry.wrap);
    if (entry.holder) {
      const left = (this.#holders.get(entry.holder) ?? 1) - 1;
      if (left > 0) this.#holders.set(entry.holder, left);
      else {
        this.#holders.delete(entry.holder);
        this.#size?.unobserve(entry.holder);
      }
    }
    if (this.#entries.size === 0) this.#halt();
  }

  /** Measures `tables` from the browser's layout and shows their columns. */
  #measure(tables: readonly HTMLTableElement[]): void {
    for (const table of tables) {
      const entry = this.#entries.get(table);
      if (!entry) continue;
      this.#watchSize(table, entry);
      // a hidden page (the other mode's) has no layout to measure: it is measured once its holder has a size
      const room = this.#probe.room(table);
      entry.stale = room <= 0;
      if (entry.stale) continue;
      unfit(table);
      entry.shown = AUTO;
      entry.measured = this.#probe.measure(table);
      if (!entry.measured) continue;
      this.#show(table, entry, screenLayout(entry.measured.columns, room, entry.measured.limits));
    }
  }

  #watchSize(table: HTMLTableElement, entry: Entry): void {
    if (entry.wrap || !this.#size) return;
    entry.wrap = table.parentElement;
    entry.holder = entry.wrap?.parentElement ?? null;
    // the wrapper's height follows the font (zoom); the holder's width is the room
    if (entry.wrap) this.#size.observe(entry.wrap);
    if (entry.holder) {
      const count = this.#holders.get(entry.holder) ?? 0;
      if (count === 0) this.#size.observe(entry.holder);
      this.#holders.set(entry.holder, count + 1);
    }
  }

  #show(table: HTMLTableElement, entry: Entry, layout: TableLayout): void {
    const key = keyOf(layout);
    if (key === entry.shown) return;
    show(table, layout);
    entry.shown = key;
  }

  #soon(): void {
    if (this.#settle !== null) clearTimeout(this.#settle);
    this.#settle = setTimeout(() => {
      this.#settle = null;
      this.#refit();
    }, SETTLE_MS);
  }

  /** Shares the measured columns again in today's room (all reads first, then the writes); a new font measures. */
  #refit(): void {
    const again: HTMLTableElement[] = [];
    const plans: [HTMLTableElement, Entry, TableLayout][] = [];
    for (const [table, entry] of this.#entries) {
      const measured = entry.measured;
      if (!measured && !entry.stale) continue;
      const room = this.#probe.room(table);
      if (room <= 0) continue;
      if (!measured || entry.stale || this.#probe.font(table) !== measured.font) again.push(table);
      else plans.push([table, entry, screenLayout(measured.columns, room, measured.limits)]);
    }
    for (const [table, entry, layout] of plans) this.#show(table, entry, layout);
    this.#measure(again);
  }

  #armIdle(): void {
    if (this.#idle !== null) clearTimeout(this.#idle);
    this.#idle = setTimeout(() => {
      this.#idle = null;
      this.#settleEdits();
    }, IDLE_MS);
  }

  /** Measures the edited tables the caret has left; the others wait for the next quiet moment. */
  #settleEdits(): void {
    const ready: HTMLTableElement[] = [];
    let waiting = false;
    for (const [table, entry] of this.#entries) {
      if (!entry.dirty) continue;
      if (busy(table)) {
        waiting = true;
        continue;
      }
      entry.dirty = false;
      // not measured yet: it is measured when it comes near the screen
      if (entry.measured) ready.push(table);
    }
    this.#measure(ready);
    if (waiting) this.#armIdle();
  }
}
