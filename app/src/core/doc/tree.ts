// A read-only tree for structured text (JSON, XML): nodes whose children are made when the node is opened, so a big
// file shows its first levels at once.

export type TreeKind =
  | "object"
  | "array"
  | "string"
  | "number"
  | "boolean"
  | "null"
  | "element"
  | "text"
  | "comment"
  | "attribute";

export type TreeNode = {
  /** The key | tag | index the node is under (`null` for the root). */
  label: string | null;
  kind: TreeKind;
  /** What is shown after the label: a value, or a count (`3 items`). */
  summary: string;
  /** Children, made on first use. `null`: a leaf. */
  children: (() => TreeNode[]) | null;
};

/** Children shown at a time under one node (a long list offers "show more"). */
export const CHUNK = 200;

/** Rows a document may open with: a smaller tree opens fully, a larger one opens the levels that fit. */
export const OPEN_BUDGET = 3000;
/** Rows "Expand all" may draw at once: past it, only the levels that fit open, so one click cannot stall the view. */
export const EXPAND_BUDGET = 10000;

/** The last "Expand all" | "Collapse all". `at` grows with each one, so the same ask applies again over rows the
 * person toggled since. */
export type Ask = { open: boolean; at: number };

/** What every row follows until the person toggles it: rows above `depth` are open. `at` = the ask it came from. */
export type Spread = { depth: number; at: number };

export function nextAsk(last: Ask | null, open: boolean): Ask {
  return { open, at: (last?.at ?? 0) + 1 };
}

/** The spread for `root`: before any ask, the levels that fit `OPEN_BUDGET`; collapse keeps only the root open. */
export function spreadOf(root: TreeNode, ask: Ask | null): Spread {
  if (ask === null) return { depth: openDepth(root, OPEN_BUDGET), at: 0 };
  return { depth: ask.open ? openDepth(root, EXPAND_BUDGET) : 1, at: ask.at };
}

/**
 * How many levels can open while the drawn rows stay within `budget` (a row draws one chunk of its children):
 * `Infinity` when the whole tree fits, never less than 1 (the root). Counts level by level and stops at the budget,
 * so it makes children only for rows that would be drawn.
 */
export function openDepth(root: TreeNode, budget: number): number {
  let level: TreeNode[] = [root];
  let rows = 1;
  let depth = 0;
  for (;;) {
    const next: TreeNode[] = [];
    for (const node of level) {
      if (node.children === null) continue;
      const shown = node.children().slice(0, CHUNK);
      rows += shown.length;
      if (rows > budget) return Math.max(1, depth);
      next.push(...shown);
    }
    if (next.length === 0) return Infinity;
    depth += 1;
    level = next;
  }
}

/**
 * Text after the label. Off, or not a string: the stored spelling. On, a string summary is the text
 * `JSON.parse` returns, and only when that result is a string. Keys are labels, not this.
 */
export function shownSummary(node: TreeNode, plain: boolean): string {
  if (!plain || node.kind !== "string") return node.summary;
  try {
    const value: unknown = JSON.parse(node.summary);
    return typeof value === "string" ? value : node.summary;
  } catch {
    return node.summary;
  }
}
