// Positions in a Visual document, for the tests that type into it.

import type { Node as PMNode } from "@milkdown/kit/prose/model";

/** The position where a top-level node starts. */
export function startOf(doc: PMNode, index: number): number {
  let pos = 0;
  for (let i = 0; i < index; i++) pos += doc.child(i).nodeSize;
  return pos;
}

/** The end of the text of the last text block inside a top-level node, or null for a node without one (a rule, an atom). */
export function endOfText(doc: PMNode, index: number): number | null {
  const start = startOf(doc, index);
  const child = doc.child(index);
  if (child.isTextblock) return start + 1 + child.content.size;
  let found: number | null = null;
  child.descendants((node, pos) => {
    if (node.isTextblock) found = start + 1 + pos + 1 + node.content.size;
  });
  return found;
}

/** The start of the text of the first text block inside a top-level node, or null for a node without one. */
export function startOfText(doc: PMNode, index: number): number | null {
  const start = startOf(doc, index);
  const child = doc.child(index);
  if (child.isTextblock) return start + 1;
  let found: number | null = null;
  child.descendants((node, pos) => {
    if (found === null && node.isTextblock) found = start + 1 + pos + 1;
  });
  return found;
}
