// Find-in-page over the rendered document: collects the text nodes, runs the pure matcher from
// ../find.ts and paints the matches with the CSS Custom Highlight API (no DOM mutation, so selection,
// links and the diff marks are unaffected).

import {
  clampIndex,
  findMatches,
  stepIndex,
  type FindMatch,
  type FindMode,
  type FindSegment,
} from "../find";

const SKIP = ".code-block__copy, .anchor, .katex-mathml, .tm-removed__bar, script, style";
const BLOCKS =
  "p, li, h1, h2, h3, h4, h5, h6, td, th, pre, blockquote, div, dd, dt, figcaption, tr, summary";

type Highlights = {
  set(name: string, value: unknown): void;
  delete(name: string): void;
};

function highlightRegistry(): Highlights | null {
  const registry = (CSS as unknown as { highlights?: Highlights }).highlights;
  return registry ?? null;
}

function makeHighlight(ranges: Range[]): unknown {
  const Ctor = (globalThis as unknown as { Highlight?: new (...r: Range[]) => unknown }).Highlight;
  return Ctor ? new Ctor(...ranges) : null;
}

export type FindState = { total: number; index: number };

export class DomFind {
  private ranges: Range[] = [];
  private index = -1;

  constructor(
    private readonly root: () => HTMLElement | null,
    private readonly scroller: () => HTMLElement | null,
    private readonly onChange: (state: FindState) => void,
  ) {}

  run(
    query: string,
    mode: FindMode,
    opts: { keepIndex?: boolean; near?: HTMLElement | null } = {},
  ): void {
    const root = this.root();
    if (!root || query.trim() === "") {
      this.clear();
      return;
    }
    const nodes: Text[] = [];
    const segments: FindSegment[] = [];
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: (node) => {
        const parent = node.parentElement;
        if (!parent || parent.closest(SKIP) || parent.closest("[hidden]")) {
          return NodeFilter.FILTER_REJECT;
        }
        return (node.nodeValue ?? "") === "" ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT;
      },
    });
    let lastBlock: Element | null = null;
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node as Text;
      const block = text.parentElement?.closest(BLOCKS) ?? null;
      nodes.push(text);
      segments.push({ text: text.data, blockStart: block !== lastBlock });
      lastBlock = block;
    }

    const matches = findMatches(segments, query, mode);
    this.ranges = matches.flatMap((m) => this.toRange(nodes, m));

    const previous = this.index;
    this.index = clampIndex(
      opts.keepIndex ? previous : this.firstIndexNear(opts.near),
      this.ranges.length,
    );
    this.paint();
    this.reveal(false);
    this.notify();
  }

  step(direction: 1 | -1): void {
    this.index = stepIndex(this.index, this.ranges.length, direction);
    this.paint();
    this.reveal(true);
    this.notify();
  }

  clear(): void {
    this.ranges = [];
    this.index = -1;
    const registry = highlightRegistry();
    registry?.delete("tm-find");
    registry?.delete("tm-find-current");
    this.notify();
  }

  private toRange(nodes: Text[], m: FindMatch): Range[] {
    const start = nodes[m.startSeg];
    const end = nodes[m.endSeg];
    if (!start || !end) return [];
    const range = document.createRange();
    range.setStart(start, Math.min(m.startOff, start.length));
    range.setEnd(end, Math.min(m.endOff, end.length));
    return [range];
  }

  /** Index of the first match at or after `near` (a located section); the first match otherwise. */
  private firstIndexNear(near: HTMLElement | null | undefined): number {
    if (!near || this.ranges.length === 0) return 0;
    const top = near.getBoundingClientRect().top - 2;
    const at = this.ranges.findIndex((r) => r.getBoundingClientRect().top >= top);
    return at < 0 ? 0 : at;
  }

  private paint(): void {
    const registry = highlightRegistry();
    if (!registry) return;
    const others = this.ranges.filter((_, i) => i !== this.index);
    const all = makeHighlight(others);
    if (all) registry.set("tm-find", all);
    const current = this.ranges[this.index];
    if (current) {
      const one = makeHighlight([current]);
      if (one) registry.set("tm-find-current", one);
    } else {
      registry.delete("tm-find-current");
    }
  }

  private reveal(smooth: boolean): void {
    const range = this.ranges[this.index];
    const scroller = this.scroller();
    if (!range || !scroller) return;
    const rect = range.getBoundingClientRect();
    const box = scroller.getBoundingClientRect();
    const target = scroller.scrollTop + (rect.top - box.top) - scroller.clientHeight / 2;
    if (rect.top < box.top + 40 || rect.bottom > box.bottom - 40 || smooth) {
      scroller.scrollTo({ top: Math.max(0, target), behavior: smooth ? "smooth" : "instant" });
    }
  }

  private notify(): void {
    this.onChange({ total: this.ranges.length, index: this.index });
  }
}
