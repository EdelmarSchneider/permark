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

/** `s` for a case-insensitive compare, every UTF-16 unit in its place (an offset in it is an offset in `s`): the dotted
 *  and dotless i go to `i` first, as `İ` lowercases to two units and Turkish pairs the cases another way (I-ı, İ-i). */
export function foldCase(s: string): string {
  return s.replace(/[İı]/g, "i").toLowerCase();
}

/** Offsets of every match of `needle` (folded by `foldCase`) in `text`, case-insensitive, in order, not overlapping. */
export function* offsetsOf(text: string, needle: string): Generator<number> {
  if (needle === "") return;
  // a fold of another length would shift the offsets; then the text is searched as it is
  const lower = foldCase(text);
  const hay = lower.length === text.length ? lower : text;
  for (let at = hay.indexOf(needle); at !== -1; at = hay.indexOf(needle, at + needle.length)) {
    yield at;
  }
}

/**
 * Elements whose text never runs into the text around them: a match stays inside one, like the editor's text blocks.
 * Lowercase `text` = a drawing's label (SVG keeps the case of its tag names), so two labels never join.
 */
const BLOCKS = new Set([
  "P",
  "LI",
  "TD",
  "TH",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "DT",
  "DD",
  "FIGCAPTION",
  "BLOCKQUOTE",
  "PRE",
  "DIV",
  "text",
]);

/** The nearest element above `node` whose text stands apart (`root` when none is). */
function blockOf(node: Node, root: HTMLElement): Element {
  for (let el = node.parentElement; el !== null && el !== root; el = el.parentElement) {
    if (BLOCKS.has(el.tagName)) return el;
  }
  return root;
}

/**
 * Every match of `query` (case-insensitive) in the text under `root`, in document order. The text nodes of one block
 * are searched as one string (bold, a link, a highlighted code token split a phrase into nodes, not into pieces), so a
 * range may start and end in different nodes; a line break, another block or the block's end ends the run.
 */
export function findIn(root: HTMLElement, query: string, limit = FIND_LIMIT): Found {
  const needle = foldCase(query);
  const ranges: Range[] = [];
  if (needle === "") return { ranges, capped: false };
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (node.nodeType === Node.TEXT_NODE) return NodeFilter.FILTER_ACCEPT;
      if (node instanceof Element && (SKIPPED.has(node.tagName) || node.hasAttribute("hidden"))) {
        return NodeFilter.FILTER_REJECT;
      }
      return node instanceof Element && node.tagName === "BR"
        ? NodeFilter.FILTER_ACCEPT
        : NodeFilter.FILTER_SKIP;
    },
  });
  // the run: its text nodes, where each starts in the joined text, the joined text
  let nodes: Text[] = [];
  let starts: number[] = [];
  let joined = "";
  let block: Element | null = null;
  // text nodes next to each other mostly share a parent: its block is looked up once
  let parent: Node | null = null;
  const flush = (): boolean => {
    let at = 0;
    for (const start of offsetsOf(joined, needle)) {
      if (ranges.length >= limit) return false;
      const end = start + needle.length;
      // matches come in order: the node holding the start only moves forward
      while (at + 1 < nodes.length && (starts[at + 1] ?? Infinity) <= start) at += 1;
      let last = at;
      while (last + 1 < nodes.length && (starts[last + 1] ?? Infinity) < end) last += 1;
      const first = nodes[at];
      const final = nodes[last];
      if (first === undefined || final === undefined) break;
      const range = document.createRange();
      range.setStart(first, start - (starts[at] ?? 0));
      range.setEnd(final, end - (starts[last] ?? 0));
      ranges.push(range);
    }
    nodes = [];
    starts = [];
    joined = "";
    return true;
  };
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if (!(node instanceof Text)) {
      if (!flush()) return { ranges, capped: true };
      block = null;
      parent = null;
      continue;
    }
    if (node.data === "") continue;
    if (node.parentNode !== parent) {
      parent = node.parentNode;
      const around = blockOf(node, root);
      if (around !== block) {
        if (!flush()) return { ranges, capped: true };
        block = around;
      }
    }
    nodes.push(node);
    starts.push(joined.length);
    joined += node.data;
  }
  return flush() ? { ranges, capped: false } : { ranges, capped: true };
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
