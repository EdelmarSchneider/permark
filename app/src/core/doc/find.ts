// Find in page: text search over the rendered document, with highlights through the CSS Custom Highlight API. The
// find bar works through a `Finder`: the page's (`pageFinder`) or the one of the editor on screen.

/** Matches kept; more are counted as "5000+". */
export const FIND_LIMIT = 5000;

const SKIPPED = new Set(["SCRIPT", "STYLE", "TEMPLATE", "NOSCRIPT"]);

export type Found = { ranges: Range[]; capped: boolean };

/** What the find bar searches: the page in View, the text of an editor. */
export type Finder = {
  /** Looks for `query` (case-insensitive) and keeps the matches; how many, `capped` past `FIND_LIMIT`. */
  search(query: string): { total: number; capped: boolean };
  /** Paints the kept matches, `current` stronger; `scroll`: brings `current` into view. */
  show(current: number, scroll: boolean): void;
  /** Removes the paint. */
  clear(): void;
  /** Calls `again` whenever what is searched changed (the matches are stale); returns what stops it. */
  watch(again: () => void): () => void;
  /** The bar closes: match `current` (`null`: none) gets the selection and the keyboard where the finder has a caret. */
  done?(current: number | null): void;
};

/** Offsets of every match of `needle` (lower case) in `text`, case-insensitive, in order, not overlapping. */
export function* offsetsOf(text: string, needle: string): Generator<number> {
  if (needle === "") return;
  // lower-casing may change the length (rare scripts); then the text is searched as it is
  const lower = text.toLowerCase();
  const hay = lower.length === text.length ? lower : text;
  for (let at = hay.indexOf(needle); at !== -1; at = hay.indexOf(needle, at + needle.length)) {
    yield at;
  }
}

/** Every match of `query` (case-insensitive) in the text nodes under `root`, in document order. */
export function findIn(root: HTMLElement, query: string, limit = FIND_LIMIT): Found {
  const needle = query.toLowerCase();
  const ranges: Range[] = [];
  if (needle === "") return { ranges, capped: false };
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (node.nodeType === Node.TEXT_NODE) return NodeFilter.FILTER_ACCEPT;
      if (node instanceof Element && (SKIPPED.has(node.tagName) || node.hasAttribute("hidden"))) {
        return NodeFilter.FILTER_REJECT;
      }
      return NodeFilter.FILTER_SKIP;
    },
  });
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const text = node.textContent ?? "";
    for (const at of offsetsOf(text, needle)) {
      if (ranges.length >= limit) return { ranges, capped: true };
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + needle.length);
      ranges.push(range);
    }
  }
  return { ranges, capped: false };
}

/** Scrolls `pane` so a match (`box`, in viewport pixels) near or past its edges stands a third down. */
export function scrollIntoPane(pane: HTMLElement, box: { top: number; bottom: number }): void {
  const edges = pane.getBoundingClientRect();
  if (box.top < edges.top + 40 || box.bottom > edges.bottom - 40) {
    pane.scrollTop += box.top - edges.top - edges.height / 3;
  }
}

/** Find over the page under `root` (it scrolls too); `painted` tells when the page was drawn again. */
export function pageFinder(root: HTMLElement, painted: (again: () => void) => () => void): Finder {
  let ranges: readonly Range[] = [];
  return {
    search: (query) => {
      const found = findIn(root, query);
      ranges = found.ranges;
      return { total: ranges.length, capped: found.capped };
    },
    show: (current, scroll) => {
      paint(ranges, current);
      const range = ranges[current];
      if (scroll && range) scrollIntoPane(root, range.getBoundingClientRect());
    },
    clear: () => {
      ranges = [];
      clearPaint();
    },
    watch: painted,
  };
}

const ALL = "pm-find";
const CURRENT = "pm-find-current";

/** Whether the webview can paint ranges without touching the DOM. */
export function highlightsSupported(): boolean {
  return typeof CSS !== "undefined" && "highlights" in CSS && typeof Highlight !== "undefined";
}

/** Paints the matches (all, and the current one stronger); without the API the current match is selected. */
export function paint(ranges: readonly Range[], current: number): void {
  if (!highlightsSupported()) {
    const range = ranges[current];
    const selection = window.getSelection();
    if (range && selection) {
      selection.removeAllRanges();
      selection.addRange(range);
    }
    return;
  }
  CSS.highlights.set(ALL, new Highlight(...ranges));
  const range = ranges[current];
  if (range) CSS.highlights.set(CURRENT, new Highlight(range));
  else CSS.highlights.delete(CURRENT);
}

export function clearPaint(): void {
  if (!highlightsSupported()) return;
  CSS.highlights.delete(ALL);
  CSS.highlights.delete(CURRENT);
}
