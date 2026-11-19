// Column widths of the tables on screen (reading view and Edit), from their measured content (`tableColumns.ts`).
// A table is measured once it comes near the screen (two layouts: at max-content and at min-content width). A resize
// shares the measured columns again without measuring, once the size settles; a font change (zoom, look, a font that
// finished loading) measures again. An edited table is measured again once editing pauses (measuring leaves the caret
// where it is), so widths never jump while typing; a column added | removed drops the widths at once (the fixed layout
// would squeeze a new column to nothing). Every pass reads all its measurements before it writes any width, so the
// page is laid out once per table and not again for each write. Without the observers (no layout to watch) tables
// keep the browser's layout.

import { TABLE_WRAP } from "./enhance";
import {
  NATURAL_TABLE,
  SCREEN_TABLE,
  measureColumns,
  ownWidths,
  screenLayout,
  showLayout,
  type ColumnSpan,
  type ScreenLimits,
  type TableLayout,
} from "./tableColumns";

/** Quiet time after the last resize before the columns are shared again. */
export const SETTLE_MS = 100;
/** Quiet time after the last edit of a table before it is measured again. */
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
  /** Came near the screen: measured (again) when needed; before that it is left alone. */
  near: boolean;
  /** Measured again once editing pauses. */
  dirty: boolean;
  /** Came near the screen while hidden (no room), or its columns changed: measured once it can be. */
  stale: boolean;
  /** The element that holds the wrapper, watched for size: its width is the room, its height follows the font. The
   *  wrapper itself is not watched: its scroll bar comes and goes with the widths set here, a loop. */
  holder: Element | null;
};

/** A table to measure in a pass, with its room. */
type Job = { table: HTMLTableElement; entry: Entry; room: number };
type Plan = [HTMLTableElement, Entry, TableLayout];

const AUTO = "auto";

function keyOf(layout: TableLayout): string {
  if (layout.kind === "auto" || layout.kind === "natural") return layout.kind;
  return `${layout.kind}:${layout.widths.map((width) => width.toFixed(1)).join(",")}`;
}

/** The table back to the browser's layout (only what the fitter set). */
function unfit(table: HTMLTableElement): void {
  table.parentElement?.classList.remove(NATURAL_TABLE);
  if (!table.classList.contains(SCREEN_TABLE)) return;
  table.querySelector(":scope > colgroup")?.remove();
  table.classList.remove(SCREEN_TABLE);
  table.style.removeProperty("width");
}

function show(table: HTMLTableElement, layout: TableLayout): void {
  unfit(table);
  showLayout(table, layout, SCREEN_TABLE);
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
  /** A font finished loading since the last pass: every measured table is measured again (its font string is the
   *  same, its glyphs are not). One event comes per font face: they share the next pass. */
  #fonts = false;
  readonly #fontsLoaded = (): void => {
    this.#fonts = true;
    this.#soon();
  };

  constructor(options: FitterOptions = {}) {
    this.#probe = options.probe ?? screenProbe;
    this.#root = options.root ?? (() => null);
  }

  /** Lays out `table` from its content once it comes near the screen. Returns what forgets it. */
  watch(table: HTMLTableElement): () => void {
    const forget = (): void => this.#forget(table);
    if (ownWidths(table) || this.#entries.has(table) || !this.#start() || !this.#view)
      return forget;
    this.#entries.set(table, {
      measured: null,
      shown: AUTO,
      near: false,
      dirty: false,
      stale: false,
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

  /**
   * The content of `table` changed (an edit; `columns` = the cells of its first row now): measured again once editing
   * pauses. A changed number of columns drops the widths at once: the browser lays the table out until then.
   */
  changed(table: HTMLTableElement, columns: number): void {
    const entry = this.#entries.get(table);
    if (!entry) return;
    const cols = table.querySelector(":scope > colgroup")?.children.length;
    if (cols !== undefined && cols !== columns) {
      unfit(table);
      entry.shown = AUTO;
      entry.measured = null;
      entry.stale = entry.near;
    }
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

  /** Measures `tables` (they came near the screen | were edited) and shows their columns. */
  #measure(tables: readonly HTMLTableElement[]): void {
    const jobs: Job[] = [];
    for (const table of tables) {
      const entry = this.#entries.get(table);
      if (!entry) continue;
      entry.near = true;
      this.#watchSize(table, entry);
      // a hidden page (the other mode's) has no layout to measure: it is measured once its holder has a size
      const room = this.#probe.room(table);
      entry.stale = room <= 0;
      if (!entry.stale) jobs.push({ table, entry, room });
    }
    this.#pass(jobs, []);
  }

  /** One pass: the tables to measure back to the browser's layout, then every measurement, then every width. */
  #pass(jobs: readonly Job[], plans: Plan[]): void {
    for (const { table, entry } of jobs) {
      unfit(table);
      entry.shown = AUTO;
    }
    for (const { table, entry, room } of jobs) {
      entry.measured = this.#probe.measure(table);
      const measured = entry.measured;
      if (measured)
        plans.push([table, entry, screenLayout(measured.columns, room, measured.limits)]);
    }
    for (const [table, entry, layout] of plans) this.#show(table, entry, layout);
  }

  #watchSize(table: HTMLTableElement, entry: Entry): void {
    if (entry.holder || !this.#size) return;
    entry.holder = table.parentElement?.parentElement ?? null;
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

  /**
   * Shares the measured columns again in today's room; a new font (or one that finished loading) measures again.
   * Only tables that came near the screen take part: the others are measured when they do.
   */
  #refit(): void {
    const fonts = this.#fonts;
    this.#fonts = false;
    const again: Job[] = [];
    const plans: Plan[] = [];
    for (const [table, entry] of this.#entries) {
      const measured = entry.measured;
      if (!measured && !entry.stale) continue;
      const room = this.#probe.room(table);
      if (room <= 0) continue;
      if (!measured || entry.stale || fonts || this.#probe.font(table) !== measured.font) {
        entry.stale = false;
        again.push({ table, entry, room });
      } else plans.push([table, entry, screenLayout(measured.columns, room, measured.limits)]);
    }
    this.#pass(again, plans);
  }

  #armIdle(): void {
    if (this.#idle !== null) clearTimeout(this.#idle);
    this.#idle = setTimeout(() => {
      this.#idle = null;
      this.#settleEdits();
    }, IDLE_MS);
  }

  /** Measures the edited tables that came near the screen (the others are measured when they do). */
  #settleEdits(): void {
    const ready: HTMLTableElement[] = [];
    for (const [table, entry] of this.#entries) {
      if (!entry.dirty) continue;
      entry.dirty = false;
      if (entry.near) ready.push(table);
    }
    this.#measure(ready);
  }
}
