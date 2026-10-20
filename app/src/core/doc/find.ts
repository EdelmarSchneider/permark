// Find in page: text search over the rendered document, with highlights through the CSS Custom Highlight API.

/** Matches kept; more are counted as "5000+". */
export const FIND_LIMIT = 5000;

const SKIPPED = new Set(["SCRIPT", "STYLE", "TEMPLATE", "NOSCRIPT"]);

export type Found = { ranges: Range[]; capped: boolean };

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
    if (text === "") continue;
    // lower-casing may change the length (rare scripts); then the text is searched as it is
    const lower = text.toLowerCase();
    const hay = lower.length === text.length ? lower : text;
    for (let at = hay.indexOf(needle); at !== -1; at = hay.indexOf(needle, at + needle.length)) {
      if (ranges.length >= limit) return { ranges, capped: true };
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + needle.length);
      ranges.push(range);
    }
  }
  return { ranges, capped: false };
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
