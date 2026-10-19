// XML text -> a read-only tree. Elements show their attributes; an element with only text shows it on its row;
// children are made when a node is opened (a big document shows its root at once).

import type { TreeNode } from "../../core/doc/tree";

export type XmlParse = { ok: true; root: TreeNode } | { ok: false };

/** Longest summary of one row. */
const SHOWN = 300;

function clip(text: string): string {
  return text.length > SHOWN ? `${text.slice(0, SHOWN)}…` : text;
}

function attributes(element: Element): string {
  return Array.from(element.attributes)
    .map((a) => `${a.name}="${a.value}"`)
    .join(" ");
}

function nodeOf(node: Node): TreeNode | null {
  switch (node.nodeType) {
    case Node.ELEMENT_NODE: {
      if (!(node instanceof Element)) return null;
      const element = node;
      const kids = Array.from(element.childNodes).filter(
        (child) => child.nodeType !== Node.TEXT_NODE || (child.textContent ?? "").trim() !== "",
      );
      const only = kids.length === 1 ? kids[0] : undefined;
      const textOnly =
        only !== undefined &&
        (only.nodeType === Node.TEXT_NODE || only.nodeType === Node.CDATA_SECTION_NODE);
      const attrs = attributes(element);
      const summary = [attrs, textOnly ? (only.textContent ?? "").trim() : ""]
        .filter((s) => s !== "")
        .join("  ");
      return {
        label: element.tagName,
        kind: "element",
        summary: clip(summary),
        children:
          kids.length === 0 || textOnly ? null : () => kids.flatMap((child) => nodeOf(child) ?? []),
      };
    }
    case Node.TEXT_NODE:
    case Node.CDATA_SECTION_NODE:
      return {
        label: null,
        kind: "text",
        summary: clip((node.textContent ?? "").trim()),
        children: null,
      };
    case Node.COMMENT_NODE:
      return {
        label: null,
        kind: "comment",
        summary: clip(`<!-- ${(node.textContent ?? "").trim()} -->`),
        children: null,
      };
    default:
      return null;
  }
}

/** The tree of an XML text, or `ok: false` when it is not well-formed. */
export function parseXml(text: string): XmlParse {
  const doc = new DOMParser().parseFromString(text, "application/xml");
  if (doc.querySelector("parsererror") || !doc.documentElement) return { ok: false };
  const root = nodeOf(doc.documentElement);
  return root === null ? { ok: false } : { ok: true, root };
}
