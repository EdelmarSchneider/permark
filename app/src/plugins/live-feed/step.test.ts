// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { pickGlow, stepGlow } from "./glow";

describe("pickGlow", () => {
  const tops = [-300, 40, 100, 300, 600];

  it("moves to the first passage below the landing line, and wraps past the last", () => {
    expect(pickGlow(tops, 1, 100)).toBe(3);
    expect(pickGlow([-50, 20], 1, 100)).toBe(0);
    expect(pickGlow([100], 1, 100)).toBe(0);
  });

  it("moves to the last passage above the landing line, and wraps before the first", () => {
    expect(pickGlow(tops, -1, 100)).toBe(1);
    expect(pickGlow([100, 300], -1, 100)).toBe(1);
    expect(pickGlow([100], -1, 100)).toBe(0);
  });

  it("finds nothing where nothing glows", () => {
    expect(pickGlow([], 1, 100)).toBeNull();
    expect(pickGlow([], -1, 100)).toBeNull();
  });
});

describe("stepGlow", () => {
  afterEach(() => {
    document.body.replaceChildren();
    vi.restoreAllMocks();
  });

  /** A pane of 500 px, scrolled to 40, with blocks standing at the given tops (pane-relative). */
  function page(blocks: { class: string; top: number }[]): {
    pane: HTMLElement;
    scrolled: number[];
    els: HTMLElement[];
  } {
    const pane = document.createElement("div");
    pane.setAttribute("data-scroller", "");
    Object.defineProperty(pane, "clientHeight", { value: 500 });
    pane.scrollTop = 40;
    pane.getBoundingClientRect = () => ({ top: 0 }) as DOMRect;
    const scrolled: number[] = [];
    pane.scrollTo = ((options: ScrollToOptions) => scrolled.push(options.top ?? -1)) as never;
    const body = document.createElement("div");
    body.className = "md-body";
    const els = blocks.map((block) => {
      const el = document.createElement("p");
      el.className = block.class;
      el.getBoundingClientRect = () => ({ top: block.top }) as DOMRect;
      body.append(el);
      return el;
    });
    pane.append(body);
    document.body.append(pane);
    return { pane, scrolled, els };
  }

  it("scrolls the next glowing passage to the landing line", () => {
    const { scrolled } = page([
      { class: "pm-glow", top: 50 },
      { class: "", top: 200 },
      { class: "pm-glow", top: 400 },
    ]);
    expect(stepGlow(1)).toBe(true);
    // landing = 20% of 500 = 100; the passage stands 400 below the edge: 40 + 400 - 100
    expect(scrolled).toEqual([340]);
    expect(stepGlow(-1)).toBe(true);
    expect(scrolled.at(-1)).toBe(0); // the first one stands above the landing line: 40 + 50 - 100 < 0
  });

  it("counts a run of glowing neighbors, and the mark of a removed passage, as one change each", () => {
    const { scrolled } = page([
      { class: "pm-glow", top: 300 },
      { class: "pm-glow", top: 340 },
      { class: "", top: 500 },
      { class: "pm-removed-mark", top: 700 },
    ]);
    expect(stepGlow(1)).toBe(true);
    expect(scrolled).toEqual([240]);
    // nothing stands above the landing line: the step before the first wraps to the mark
    expect(stepGlow(-1)).toBe(true);
    expect(scrolled.at(-1)).toBe(640);
  });

  it("reaches the glow a text viewer draws", () => {
    const { pane, scrolled } = page([]);
    const glows = document.createElement("div");
    glows.className = "glows";
    const glow = document.createElement("div");
    glow.className = "glow";
    glow.getBoundingClientRect = () => ({ top: 260 }) as DOMRect;
    glows.append(glow);
    pane.append(glows);
    expect(stepGlow(1)).toBe(true);
    expect(scrolled).toEqual([200]);
  });

  it("is false when nothing glows or no document is shown", () => {
    page([{ class: "", top: 10 }]);
    expect(stepGlow(1)).toBe(false);
    document.body.replaceChildren();
    expect(stepGlow(1)).toBe(false);
  });
});
