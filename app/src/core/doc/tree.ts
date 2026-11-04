// A read-only tree for structured text (JSON, XML): nodes whose children are made when the node is opened, so a big
// file shows its first level at once.

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
