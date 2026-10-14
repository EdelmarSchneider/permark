// The document outline (docs/design/v0.2.md section 2): the tree of a document's headings, the section being
// read, collapsing, and the keyboard. Pure; DocView reads the DOM and the store keeps the result.

/** A top-level block of the rendered document, as the outline sees it. */
export type BlockInfo = {
  /** 1 to 6 for a heading, null for any other block. */
  level: number | null;
  /** The heading's text (unused for other blocks). */
  text: string;
  /** The block carries a live-change mark: a changed block, or a marker for removed blocks. */
  changed: boolean;
};

export type OutlineItem = {
  /** Position among the kept headings, in document order. */
  index: number;
  /** 1 to 6. */
  level: number;
  text: string;
  /** Nesting depth: 0 for a top heading, one more than its parent. The list is indented by this. */
  depth: number;
  parent: number | null;
  hasChildren: boolean;
  /** Where its subtree stops: the headings after `index` and before `end` sit below this one. */
  end: number;
  /** The blocks of this section (up to the next heading) changed in the last update. */
  changed: boolean;
  /** A section below this one changed (shown on it while it is collapsed). */
  subtreeChanged: boolean;
  /** Identifies the heading across rebuilds: its text, numbered when the same text appears again. */
  key: string;
};

export type OutlineModel = {
  items: OutlineItem[];
  /** Headings in the document, kept or not. */
  total: number;
};

export const EMPTY_OUTLINE: OutlineModel = { items: [], total: 0 };

/** A longer list is cut here and says how many there are. */
export const MAX_HEADINGS = 1000;

/** Builds the outline from the document's top-level blocks, in order. */
export function buildOutline(blocks: readonly BlockInfo[]): OutlineModel {
  const items: OutlineItem[] = [];
  const open: OutlineItem[] = []; // the headings whose subtree is still growing
  const seen = new Map<string, number>();
  let total = 0;
  // the section the next plain block belongs to: null above the first heading and below the cut
  let section: OutlineItem | null = null;

  for (const block of blocks) {
    if (block.level === null) {
      if (block.changed && section) section.changed = true;
      continue;
    }
    total++;
    if (items.length >= MAX_HEADINGS) {
      section = null;
      continue;
    }
    while (open.length > 0 && open[open.length - 1].level >= block.level) {
      const done = open.pop();
      if (done) done.end = items.length;
    }
    const n = (seen.get(block.text) ?? 0) + 1;
    seen.set(block.text, n);
    const item: OutlineItem = {
      index: items.length,
      level: block.level,
      text: block.text,
      depth: open.length,
      parent: open.length > 0 ? open[open.length - 1].index : null,
      hasChildren: false,
      end: items.length + 1,
      changed: block.changed,
      subtreeChanged: false,
      key: n === 1 ? block.text : `${block.text}\u0000${n}`,
    };
    items.push(item);
    open.push(item);
    section = item;
  }
  for (const rest of open) rest.end = items.length;

  for (const item of items) item.hasChildren = item.end > item.index + 1;
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if ((item.changed || item.subtreeChanged) && item.parent !== null) {
      items[item.parent].subtreeChanged = true;
    }
  }
  return { items, total };
}

/** The headings that show, in order: everything below a collapsed heading is skipped. */
export function visibleRows(
  items: readonly OutlineItem[],
  collapsed: ReadonlySet<string>,
): number[] {
  const rows: number[] = [];
  let i = 0;
  while (i < items.length) {
    rows.push(i);
    const item = items[i];
    i = item.hasChildren && collapsed.has(item.key) ? item.end : i + 1;
  }
  return rows;
}

const NONE: ReadonlySet<string> = new Set();

/**
 * What is collapsed in each document, for the session. It is kept by heading text, so a live reload that
 * rebuilds the outline keeps it (a renamed heading starts open again). Only the most recently used
 * documents are remembered.
 */
export class CollapseMemory {
  private byDoc = new Map<string, ReadonlySet<string>>();

  constructor(private readonly limit = 64) {}

  get(path: string): ReadonlySet<string> {
    return this.byDoc.get(path) ?? NONE;
  }

  /** Collapses or expands one heading and returns the document's new set (never the old object, so a change is seen). */
  set(path: string, key: string, collapse: boolean): ReadonlySet<string> {
    const next = new Set(this.get(path));
    if (collapse) next.add(key);
    else next.delete(key);
    this.byDoc.delete(path); // re-inserted last: the map's order is the order of use
    if (next.size > 0) {
      this.byDoc.set(path, next);
      while (this.byDoc.size > this.limit) {
        const oldest = this.byDoc.keys().next().value;
        if (oldest === undefined) break;
        this.byDoc.delete(oldest);
      }
    }
    return next.size > 0 ? next : NONE;
  }

  clear(): void {
    this.byDoc.clear();
  }
}

/**
 * The row that stands for a heading: the heading itself, or when a collapsed heading hides it, the outermost
 * collapsed heading above it (that is the row to mark as the section being read, or to keep the cursor on).
 */
export function visibleAncestor(
  items: readonly OutlineItem[],
  collapsed: ReadonlySet<string>,
  index: number | null,
): number | null {
  if (index === null || !items[index]) return null;
  let found = index;
  for (let p = items[index].parent; p !== null; p = items[p].parent) {
    if (collapsed.has(items[p].key)) found = p;
  }
  return found;
}

/**
 * The section being read: the last heading whose top is at or above `line` (both measured from the top of
 * the scroller). Heading tops only grow down the page, so this is a binary search over `count` headings,
 * which keeps a 1,000-heading document cheap on every scroll frame. At the very bottom of the page the last
 * heading can never reach the line, so `bottomEdge` (the scroller's height) also lets the last heading that
 * is in view count. Null above the first heading.
 */
export function currentHeading(
  count: number,
  topOf: (index: number) => number,
  view: { line: number; bottomEdge?: number },
): number | null {
  const lastWhere = (test: (top: number) => boolean): number => {
    let low = 0;
    let high = count - 1;
    let found = -1;
    while (low <= high) {
      const mid = (low + high) >> 1;
      if (test(topOf(mid))) {
        found = mid;
        low = mid + 1;
      } else {
        high = mid - 1;
      }
    }
    return found;
  };
  let found = lastWhere((top) => top <= view.line);
  const edge = view.bottomEdge;
  if (edge !== undefined)
    found = Math.max(
      found,
      lastWhere((top) => top < edge),
    );
  return found < 0 ? null : found;
}

export type OutlineNav = {
  /** The heading to put the cursor on. */
  cursor: number;
  /** A heading to collapse or expand. */
  toggle?: { index: number; expand: boolean };
  /** A heading to scroll to. */
  open?: number;
};

/** What a key does with the cursor on `cursor`; null when it is not an outline key. */
export function navigate(
  items: readonly OutlineItem[],
  rows: readonly number[],
  cursor: number,
  collapsed: ReadonlySet<string>,
  key: string,
): OutlineNav | null {
  if (rows.length === 0) return null;
  const at = visibleAncestor(items, collapsed, cursor) ?? rows[0];
  const pos = Math.max(0, rows.indexOf(at));
  const item = items[at];
  const closed = item.hasChildren && collapsed.has(item.key);
  switch (key) {
    case "ArrowDown":
      return { cursor: rows[Math.min(pos + 1, rows.length - 1)] };
    case "ArrowUp":
      return { cursor: rows[Math.max(pos - 1, 0)] };
    case "Home":
      return { cursor: rows[0] };
    case "End":
      return { cursor: rows[rows.length - 1] };
    case "Enter":
    case " ":
      return { cursor: at, open: at };
    case "ArrowRight":
      if (closed) return { cursor: at, toggle: { index: at, expand: true } };
      return { cursor: item.hasChildren ? rows[Math.min(pos + 1, rows.length - 1)] : at };
    case "ArrowLeft":
      if (item.hasChildren && !closed) return { cursor: at, toggle: { index: at, expand: false } };
      return { cursor: item.parent ?? at };
    default:
      return null;
  }
}
