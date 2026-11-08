// HTML written in a line (`Press <kbd>Ctrl</kbd>`, `H<sub>2</sub>O`, `a<br>b`): one inline piece per element, kept as
// written, drawn as View draws it. The look is only ever View's element for that source (backend HTML, sanitized),
// never the person's HTML: a piece View has no element for (dropped by the sanitizer, not yet rendered) shows its
// source as text.

import type { Ctx } from "@milkdown/kit/ctx";
import type { Html, Nodes, PhrasingContent, Root } from "mdast";
import { lockedHostCtx } from "./lockedView";

/** An opening tag alone: its element's name. */
const OPEN = /^<([a-zA-Z][a-zA-Z0-9-]*)(?:\s[^>]*)?>$/;
/** A tag that closes itself (`<br>`, `<br/>`, `<img ... />`). */
const ALONE = /^<(?:(?:br|wbr)\b[^>]*|[a-zA-Z][a-zA-Z0-9-]*(?:\s[^>]*)?\/)>$/i;

function closes(node: PhrasingContent, name: string): boolean {
  return node.type === "html" && new RegExp(`^</${name}\\s*>$`, "i").test(node.value);
}

function opens(node: PhrasingContent, name: string): boolean {
  return node.type === "html" && OPEN.exec(node.value)?.[1]?.toLowerCase() === name;
}

/** The index of the tag closing the element opened at `at` (same-name elements nested inside counted); -1: none. */
function closingOf(children: readonly PhrasingContent[], at: number, name: string): number {
  let depth = 0;
  for (let i = at; i < children.length; i++) {
    const child = children[i];
    if (child === undefined) continue;
    if (opens(child, name)) depth += 1;
    else if (closes(child, name)) {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** One line's children with each element written as HTML made one piece of its exact source. */
function piecesIn(children: readonly PhrasingContent[], text: string): PhrasingContent[] {
  const out: PhrasingContent[] = [];
  for (let i = 0; i < children.length; i++) {
    const child = children[i];
    if (child === undefined) continue;
    const name = child.type === "html" ? OPEN.exec(child.value)?.[1]?.toLowerCase() : undefined;
    const alone = child.type === "html" && ALONE.test(child.value);
    const end = alone ? i : name === undefined ? -1 : closingOf(children, i, name);
    const start = child.position?.start;
    const stop = children[end]?.position?.end;
    if (end < 0 || start?.offset === undefined || stop?.offset === undefined) {
      out.push(child);
      continue;
    }
    const value = text.slice(start.offset, stop.offset);
    out.push({ type: "inlineSource", kind: "html", value, position: { start, end: stop } });
    i = end;
  }
  return out;
}

/** Every element written as HTML in a line of `tree` (read from `text`) becomes its piece; other HTML stays. */
export function htmlPieces(tree: Root, text: string): void {
  const visit = (node: Nodes): void => {
    if (!("children" in node)) return;
    node.children.forEach(visit);
    switch (node.type) {
      case "paragraph":
      case "heading":
      case "tableCell":
      case "emphasis":
      case "strong":
      case "delete":
      case "link":
      case "linkReference":
        if (node.children.some((child: PhrasingContent): child is Html => child.type === "html")) {
          node.children = piecesIn(node.children, text);
        }
        break;
      default:
        break;
    }
  };
  visit(tree);
}

/** View's element for the piece `source` (backend HTML, its block markers gone); null: none known. */
export function htmlLook(ctx: Ctx, source: string): HTMLElement | null {
  const html = ctx.get(lockedHostCtx.key).inline?.(source) ?? null;
  if (html === null) return null;
  const template = document.createElement("template");
  template.innerHTML = html;
  const element = template.content.firstElementChild;
  if (!(element instanceof HTMLElement) || template.content.childNodes.length !== 1) return null;
  for (const marked of [element, ...element.querySelectorAll("[data-b]")]) {
    marked.removeAttribute("data-b");
  }
  return element;
}
