// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NATURAL_TABLE, SCREEN_TABLE, type ColumnSpan } from "./tableColumns";
import { SETTLE_MS, IDLE_MS, TableFitter, type TableProbe } from "./tableFitter";

type Watcher = {
  observed: Set<Element>;
  fire(targets: Element[]): void;
};
let views: Watcher[] = [];
let sizes: Watcher[] = [];

function fakeObserver(into: () => Watcher[], entry: (el: Element) => object) {
  return class {
    #watch: Watcher;
    constructor(callback: (entries: object[], observer: unknown) => void) {
      const watch: Watcher = {
        observed: new Set(),
        fire: (targets) =>
          callback(
            targets.map((el) => entry(el)),
            this,
          ),
      };
      this.#watch = watch;
      into().push(watch);
    }
    observe(el: Element): void {
      this.#watch.observed.add(el);
    }
    unobserve(el: Element): void {
      this.#watch.observed.delete(el);
    }
    disconnect(): void {
      this.#watch.observed.clear();
    }
  };
}

/** A probe with layout given by the test (jsdom draws none). */
function probe(columns: ColumnSpan[], room: number) {
  // `fitted`: the tables showing measured widths at each measurement (a pass measures before it writes any)
  const state = { columns, room, font: "16px sans", measured: 0, fitted: [] as number[] };
  const fake: TableProbe = {
    measure: () => {
      state.measured += 1;
      state.fitted.push(document.querySelectorAll(`.${SCREEN_TABLE}`).length);
      return { columns: state.columns, limits: { short: 100, cap: 330 }, font: state.font };
    },
    font: () => state.font,
    room: () => state.room,
  };
  return { state, fake };
}

function table(): { wrap: HTMLElement; table: HTMLTableElement } {
  const wrap = document.createElement("div");
  wrap.className = "table-wrap";
  wrap.innerHTML = "<table><tbody><tr><td>1</td><td>long</td></tr></tbody></table>";
  document.body.append(wrap);
  const found = wrap.querySelector("table");
  if (!found) throw new Error("no table");
  return { wrap, table: found };
}

const colWidths = (el: HTMLTableElement): string[] =>
  Array.from(
    el.querySelectorAll(":scope > colgroup > col"),
    (col) => (col as HTMLElement).style.width,
  );

const LONG = [
  { min: 60, max: 70 },
  { min: 90, max: 2000 },
];

describe("TableFitter", () => {
  beforeEach(() => {
    views = [];
    sizes = [];
    vi.useFakeTimers();
    vi.stubGlobal(
      "IntersectionObserver",
      fakeObserver(
        () => views,
        (target) => ({ target, isIntersecting: true }),
      ),
    );
    vi.stubGlobal(
      "ResizeObserver",
      fakeObserver(
        () => sizes,
        (target) => ({ target }),
      ),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  it("measures a table only once it comes into view, then lays its columns out", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { table: t } = table();
    fitter.watch(t);
    expect(state.measured).toBe(0);
    expect(t.querySelector("colgroup")).toBeNull();
    views[0]?.fire([t]);
    expect(state.measured).toBe(1);
    expect(t.classList.contains(SCREEN_TABLE)).toBe(true);
    expect(colWidths(t)).toEqual(["17.5%", "82.5%"]);
    fitter.stop();
  });

  it("shares the columns again on a resize from what it measured, once the size settles", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { wrap, table: t } = table();
    fitter.watch(t);
    views[0]?.fire([t]);
    // the holder only: the wrapper's scroll bar follows the widths set here
    expect([...(sizes[0]?.observed ?? [])]).toEqual([document.body]);
    // a table wider than the room after a narrower column: it scrolls, the long text capped
    state.room = 120;
    sizes[0]?.fire([document.body]);
    sizes[0]?.fire([document.body]);
    expect(t.classList.contains(SCREEN_TABLE)).toBe(true);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(state.measured).toBe(1);
    expect(t.style.width).toBe("400px");
    expect(colWidths(t)).toEqual(["17.5%", "82.5%"]);
    // wide enough for every line: the table keeps its own width
    state.room = 5000;
    sizes[0]?.fire([document.body]);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(t.querySelector("colgroup")).toBeNull();
    expect(t.classList.contains(SCREEN_TABLE)).toBe(false);
    expect(t.style.width).toBe("");
    expect(wrap.classList.contains(NATURAL_TABLE)).toBe(true);
    expect(state.measured).toBe(1);
    fitter.stop();
  });

  it("measures again when the table's font changed (zoom, look)", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { table: t } = table();
    fitter.watch(t);
    views[0]?.fire([t]);
    state.font = "20px sans";
    sizes[0]?.fire([document.body]);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(state.measured).toBe(2);
    fitter.stop();
  });

  it("measures an edited table again once editing pauses, with the caret still in it", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { table: t } = table();
    const editor = document.createElement("div");
    editor.tabIndex = 0;
    document.body.append(editor);
    editor.append(t.parentElement as HTMLElement);
    fitter.watch(t);
    views[0]?.fire([t]);
    editor.focus();
    const cell = t.querySelector("td");
    document.getSelection()?.collapse(cell?.firstChild ?? null, 1);
    for (let key = 0; key < 5; key += 1) {
      fitter.changed(t, 2);
      vi.advanceTimersByTime(IDLE_MS / 2);
    }
    expect(state.measured).toBe(1);
    expect(colWidths(t)).toHaveLength(2);
    vi.advanceTimersByTime(IDLE_MS / 2);
    expect(state.measured).toBe(2);
    expect(document.getSelection()?.anchorNode).toBe(cell?.firstChild);
    fitter.stop();
  });

  it("drops the widths at once when a column is added, and measures once editing pauses", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { wrap, table: t } = table();
    fitter.watch(t);
    views[0]?.fire([t]);
    expect(colWidths(t)).toHaveLength(2);
    t.rows[0]?.insertCell();
    fitter.changed(t, 3);
    expect(t.querySelector("colgroup")).toBeNull();
    expect(t.classList.contains(SCREEN_TABLE)).toBe(false);
    state.columns = [...LONG, { min: 60, max: 70 }];
    vi.advanceTimersByTime(IDLE_MS - 1);
    expect(state.measured).toBe(1);
    vi.advanceTimersByTime(1);
    expect(state.measured).toBe(2);
    expect(colWidths(t)).toHaveLength(3);
    expect(wrap.classList.contains(NATURAL_TABLE)).toBe(false);
    fitter.stop();
  });

  it("measures the tables near the screen in one pass after fonts finished loading", () => {
    const fonts = new EventTarget();
    Object.defineProperty(document, "fonts", { value: fonts, configurable: true });
    try {
      const { state, fake } = probe(LONG, 400);
      const fitter = new TableFitter({ probe: fake });
      const near = [table().table, table().table, table().table];
      const far = table().table;
      for (const t of [...near, far]) fitter.watch(t);
      views[0]?.fire(near);
      expect(state.measured).toBe(3);
      state.fitted = [];
      // one event per font face
      for (let face = 0; face < 3; face += 1) fonts.dispatchEvent(new Event("loadingdone"));
      expect(state.measured).toBe(3);
      vi.advanceTimersByTime(SETTLE_MS);
      // the far table waits until it comes near; every measurement before any width is written again
      expect(state.measured).toBe(6);
      expect(state.fitted).toEqual([0, 0, 0]);
      for (const t of near) expect(t.classList.contains(SCREEN_TABLE)).toBe(true);
      expect(far.classList.contains(SCREEN_TABLE)).toBe(false);
      fitter.stop();
    } finally {
      Reflect.deleteProperty(document, "fonts");
    }
  });

  it("measures a table again once a cell was drawn anew after it was measured (a formula mounted), not before it is near", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { table: t } = table();
    fitter.watch(t);
    // drawn before the table came near: it is measured with what it holds then
    fitter.redrawn(t);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(state.measured).toBe(0);
    // measured while a cell still held the formula's source text, then the formula was drawn (narrower)
    views[0]?.fire([t]);
    expect(state.measured).toBe(1);
    expect(colWidths(t)).toEqual(["17.5%", "82.5%"]);
    state.columns = [
      { min: 20, max: 30 },
      { min: 90, max: 2000 },
    ];
    // a table holds several formulas: one pass for all of them, once they settle
    for (let cell = 0; cell < 3; cell += 1) fitter.redrawn(t);
    expect(state.measured).toBe(1);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(state.measured).toBe(2);
    expect(colWidths(t)).not.toEqual(["17.5%", "82.5%"]);
    fitter.stop();
  });

  it("measures a table that came near while hidden once it is shown", () => {
    const { state, fake } = probe(LONG, 0);
    const fitter = new TableFitter({ probe: fake });
    const { wrap, table: t } = table();
    fitter.watch(t);
    views[0]?.fire([t]);
    expect(state.measured).toBe(0);
    expect(sizes[0]?.observed.has(document.body)).toBe(true);
    state.room = 400;
    sizes[0]?.fire([document.body]);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(state.measured).toBe(1);
    expect(t.classList.contains(SCREEN_TABLE)).toBe(true);
    fitter.stop();
  });

  it("leaves a table with its own column widths alone", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { table: t } = table();
    t.prepend(document.createElement("colgroup"));
    fitter.watch(t);
    views[0]?.fire([t]);
    expect(state.measured).toBe(0);
    expect(t.classList.contains(SCREEN_TABLE)).toBe(false);
    fitter.stop();
  });

  it("forgets a table it no longer watches and stops watching once none is left", () => {
    const { fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { wrap } = table();
    const forget = fitter.watchIn(wrap);
    const t = wrap.querySelector("table");
    expect(t && views[0]?.observed.has(t)).toBe(true);
    if (t) views[0]?.fire([t]);
    forget();
    expect(views[0]?.observed.size).toBe(0);
    expect(sizes[0]?.observed.size).toBe(0);
  });
});
