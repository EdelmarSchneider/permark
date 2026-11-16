// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FIT_TABLE, NATURAL_TABLE, type ColumnSpan } from "./tableColumns";
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
  const state = { columns, room, font: "16px sans", measured: 0 };
  const fake: TableProbe = {
    measure: () => {
      state.measured += 1;
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
    expect(t.classList.contains(FIT_TABLE)).toBe(true);
    expect(colWidths(t)).toEqual(["17.5%", "82.5%"]);
    fitter.stop();
  });

  it("shares the columns again on a resize from what it measured, once the size settles", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { wrap, table: t } = table();
    fitter.watch(t);
    views[0]?.fire([t]);
    expect(sizes[0]?.observed.has(wrap)).toBe(true);
    // a table wider than the room after a narrower column: it scrolls, the long text capped
    state.room = 120;
    sizes[0]?.fire([wrap]);
    sizes[0]?.fire([wrap]);
    expect(t.classList.contains(FIT_TABLE)).toBe(true);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(state.measured).toBe(1);
    expect(t.style.width).toBe("400px");
    expect(colWidths(t)).toEqual(["17.5%", "82.5%"]);
    // wide enough for every line: the table keeps its own width
    state.room = 5000;
    sizes[0]?.fire([wrap]);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(t.querySelector("colgroup")).toBeNull();
    expect(t.classList.contains(FIT_TABLE)).toBe(false);
    expect(t.style.width).toBe("");
    expect(wrap.classList.contains(NATURAL_TABLE)).toBe(true);
    expect(state.measured).toBe(1);
    fitter.stop();
  });

  it("measures again when the table's font changed (zoom, look)", () => {
    const { state, fake } = probe(LONG, 400);
    const fitter = new TableFitter({ probe: fake });
    const { wrap, table: t } = table();
    fitter.watch(t);
    views[0]?.fire([t]);
    state.font = "20px sans";
    sizes[0]?.fire([wrap]);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(state.measured).toBe(2);
    fitter.stop();
  });

  it("measures an edited table again once idle and only after the caret left it", () => {
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
    for (let key = 0; key < 5; key += 1) fitter.changed(t);
    vi.advanceTimersByTime(IDLE_MS * 3);
    expect(state.measured).toBe(1);
    document.getSelection()?.collapse(document.body, 0);
    vi.advanceTimersByTime(IDLE_MS);
    expect(state.measured).toBe(2);
    fitter.stop();
  });

  it("measures a table that came near while hidden once it is shown", () => {
    const { state, fake } = probe(LONG, 0);
    const fitter = new TableFitter({ probe: fake });
    const { wrap, table: t } = table();
    fitter.watch(t);
    views[0]?.fire([t]);
    expect(state.measured).toBe(0);
    expect(sizes[0]?.observed.has(wrap)).toBe(true);
    state.room = 400;
    sizes[0]?.fire([wrap]);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(state.measured).toBe(1);
    expect(t.classList.contains(FIT_TABLE)).toBe(true);
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
    expect(t.classList.contains(FIT_TABLE)).toBe(false);
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
