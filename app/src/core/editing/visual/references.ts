// References to definitions elsewhere in the document: footnote references `[^a]` and reference-style links
// (`[text][ref]`, `[text][]`, `[text]`). The editor reads one block at a time, where such a reference alone reads as
// text (written escaped, it would stop being one). So every reading counts the document's labels as defined
// (`definedSyntax`: the parser's own label matching, as View's: case-insensitive, inner whitespace folded), and each
// reference the read text does not define itself becomes an inline piece kept as written (`inlineSource.ts`), drawn
// as View draws it: a footnote's superscript number, a link with its definition's href. The labels are the ones read
// at load: a definition edited later changes the file, never what a block being edited reads as.

import { remarkCtx } from "@milkdown/kit/core";
import type { Ctx } from "@milkdown/kit/ctx";
import { sanitizeLinkHref } from "@milkdown/kit/preset/commonmark";
import { $ctx } from "@milkdown/kit/utils";
import type { FootnoteReference, LinkReference, Nodes, PhrasingContent, Root } from "mdast";
import type { Construct, Extension, ParseContext } from "micromark-util-types";
import type { Core, MdastNode } from "./core";

declare module "micromark-util-types" {
  interface ParseContext {
    /** Footnote labels defined so far (the GFM footnote syntax's own list). */
    gfmFootnotes?: Array<string>;
  }
}

export type LinkTarget = { url: string; title: string | null };

/** The definitions of a document. */
export type References = {
  /** Link definitions by mdast identifier; the first one of a label wins, as in View. */
  links: ReadonlyMap<string, LinkTarget>;
  /** mdast identifiers of the footnote definitions. */
  notes: ReadonlySet<string>;
};

export const NO_REFERENCES: References = { links: new Map(), notes: new Set() };

export const referencesCtx = $ctx<References, "pmReferences">(NO_REFERENCES, "pmReferences");

/** The definitions every reading of `core` counts as given from now on (a document's, read at its load). */
export function useReferences(core: Core, references: References): void {
  core.editor.action((ctx) => ctx.set(referencesCtx.key, references));
}

/** The definitions in `tree` (a whole text as parsed, at any depth). */
export function referencesOf(tree: MdastNode): References {
  const links = new Map<string, LinkTarget>();
  const notes = new Set<string>();
  const visit = (node: MdastNode): void => {
    const id = node.identifier;
    if (node.type === "definition" && typeof id === "string" && typeof node.url === "string") {
      if (!links.has(id)) {
        links.set(id, { url: node.url, title: typeof node.title === "string" ? node.title : null });
      }
    } else if (node.type === "footnoteDefinition" && typeof id === "string") notes.add(id);
    node.children?.forEach(visit);
  };
  visit(tree);
  return links.size === 0 && notes.size === 0 ? NO_REFERENCES : { links, notes };
}

/**
 * The document's labels into each parse, before its first block is read. The parser keeps the labels it has seen
 * defined (`defined`, `gfmFootnotes`, in its normalized form: mdast's identifier upper-cased) and tells a reference
 * from text by them; a check at the start of the first line (tried at every line start, never taking a character)
 * gives them.
 */
export function definedSyntax(current: () => References): Extension {
  const given = new WeakSet<ParseContext>();
  const labels: Construct = {
    name: "pmDocumentLabels",
    tokenize(_effects, _ok, nok) {
      if (!given.has(this.parser)) {
        given.add(this.parser);
        const references = current();
        for (const id of references.links.keys()) this.parser.defined.push(id.toUpperCase());
        const notes = (this.parser.gfmFootnotes ??= []);
        for (const id of references.notes) notes.push(id.toUpperCase());
      }
      return nok;
    },
  };
  return { document: { null: labels } };
}

/** Identifiers `tree` defines itself (a paste with its definitions: the editor's own links | footnotes take those). */
function ownDefinitions(tree: Root): { links: Set<string>; notes: Set<string> } {
  const own = { links: new Set<string>(), notes: new Set<string>() };
  const visit = (node: Nodes): void => {
    if (node.type === "definition") own.links.add(node.identifier);
    if (node.type === "footnoteDefinition") own.notes.add(node.identifier);
    if ("children" in node) node.children.forEach(visit);
  };
  visit(tree);
  return own;
}

/**
 * A reference the read text does not define as its piece (its exact source); a picture by reference as its text
 * (the editor has no piece for it; blocks holding one are kept as written).
 */
function pieceOf(
  node: PhrasingContent,
  own: { links: Set<string>; notes: Set<string> },
  text: string,
): PhrasingContent | null {
  const from = node.position?.start.offset;
  const to = node.position?.end.offset;
  if (from === undefined || to === undefined) return null;
  const value = text.slice(from, to);
  const position = node.position;
  switch (node.type) {
    case "footnoteReference":
      return own.notes.has(node.identifier)
        ? null
        : { type: "inlineSource", kind: "footnote", value, position };
    case "linkReference":
      return own.links.has(node.identifier)
        ? null
        : { type: "inlineSource", kind: "link", value, position };
    case "imageReference":
      return own.links.has(node.identifier) ? null : { type: "text", value, position };
    default:
      return null;
  }
}

/** Every reference in `tree` (read from `text`) the text does not define itself becomes its piece (`pieceOf`). */
export function referencePieces(tree: Root, text: string): void {
  const own = ownDefinitions(tree);
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
        node.children = node.children.map((child) => pieceOf(child, own, text) ?? child);
        break;
      default:
        break;
    }
  };
  visit(tree);
}

type Found = FootnoteReference | LinkReference;

/** What each source read so far holds, per set of definitions (a piece is drawn again on every pause of its field). */
const readings = new WeakMap<References, Map<string, Found[]>>();

/** The outermost references `source` holds when read with the document's definitions, in order. */
function referencesIn(ctx: Ctx, source: string): Found[] {
  const references = ctx.get(referencesCtx.key);
  let known = readings.get(references);
  if (known === undefined) {
    known = new Map();
    readings.set(references, known);
  }
  let found = known.get(source);
  if (found === undefined) {
    const list: Found[] = [];
    const visit = (node: Nodes): void => {
      if (node.type === "footnoteReference" || node.type === "linkReference") list.push(node);
      else if ("children" in node) node.children.forEach(visit);
    };
    try {
      visit(ctx.get(remarkCtx).parse(source));
    } catch {
      // unreadable: no reference
    }
    found = list;
    known.set(source, found);
  }
  return found;
}

/** Identifiers of the footnotes `source` refers to, in order (footnotes inside a link's text too). */
export function footnoteIds(ctx: Ctx, source: string): string[] {
  if (!source.includes("[^") || ctx.get(referencesCtx.key).notes.size === 0) return [];
  const ids: string[] = [];
  const visit = (node: PhrasingContent): void => {
    if (node.type === "footnoteReference") ids.push(node.identifier);
    else if ("children" in node) node.children.forEach(visit);
  };
  referencesIn(ctx, source).forEach(visit);
  return ids;
}

/** View's superscript of a footnote reference: its number in the document; null when `source` is none. */
export function footnoteLook(
  ctx: Ctx,
  source: string,
  numbers: ReadonlyMap<string, number>,
): HTMLElement | null {
  const found = referencesIn(ctx, source)[0];
  const number = found?.type === "footnoteReference" ? numbers.get(found.identifier) : undefined;
  if (found?.type !== "footnoteReference" || number === undefined) return null;
  const sup = document.createElement("sup");
  sup.className = "footnote-ref";
  const anchor = document.createElement("a");
  anchor.setAttribute("href", `#fn-${found.label ?? found.identifier}`);
  anchor.setAttribute("data-footnote-ref", "");
  anchor.textContent = String(number);
  sup.append(anchor);
  return sup;
}

/** The text a reader sees of `nodes` (markup left out). */
function shownText(nodes: readonly PhrasingContent[]): string {
  return nodes
    .map((node) =>
      "children" in node ? shownText(node.children) : "value" in node ? node.value : "",
    )
    .join("");
}

/** View's link of a reference-style link: its text, its definition's href (when safe) and title; null: none. */
export function linkLook(ctx: Ctx, source: string): HTMLElement | null {
  const found = referencesIn(ctx, source)[0];
  const target =
    found?.type === "linkReference" ? ctx.get(referencesCtx.key).links.get(found.identifier) : null;
  if (found?.type !== "linkReference" || !target) return null;
  const anchor = document.createElement("a");
  const href = sanitizeLinkHref(target.url);
  if (href !== "") anchor.setAttribute("href", href);
  if (target.title !== null) anchor.title = target.title;
  anchor.textContent = shownText(found.children);
  return anchor;
}
