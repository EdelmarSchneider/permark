// What diagram embeds (Mermaid, PlantUML, Graphviz) share: one render queue (a preview and a document never draw at
// the same time), a small result cache, the page's palette, and the cleaning every drawing gets before it enters the
// page.

import { isLocalHref } from "./decorate";

/** A drawing that could not be made, with the words that say why (the engine's, or why it was not tried). */
export type DrawFailure = { problem: string };

const SVG_NS = "http://www.w3.org/2000/svg";
const XLINK_NS = "http://www.w3.org/1999/xlink";
/** Addresses the page's own link handling follows besides project files: web, mail, a place in the page. */
const PAGE_LINK = /^(?:https?:|mailto:|#)/i;
/** `//host`, `\\server`, `/\host`: the web view reads a backslash as a slash, so any mix leaves the machine. */
const NETWORK = /^[\\/]{2}/;
/** A browser drops tabs and line breaks from inside an address (`ht\ttps:` is `https:`): no reading of one is safe. */
const CONTROL = /[\u0000-\u001f\u007f]/;
// the scheme is read without blanks (`java script:` counts as `javascript:`): an address never passes by a blank alone
const BLANKS = /\s/g;
/** An `id` a reference names as written: a browser decodes `%` escapes and drops tabs before it looks one up. */
const PLAIN_ID = /^[\p{L}\p{N}_.:-]+$/u;
/** What may come before the root element: blanks, the XML declaration, processing instructions, comments, a doctype. */
const PROLOG = /^(?:\s+|<\?[\s\S]*?\?>|<!--[\s\S]*?-->|<!DOCTYPE[^[>]*(?:\[[\s\S]*?\])?\s*>)*/i;
/** The root's start tag: its attributes one by one (a quoted value may hold a `>`), then the end of the tag. */
const ROOT_TAG = /^<svg((?:\s+[^\s=/>]+\s*=\s*(?:"[^"]*"|'[^']*'))*)\s*\/?>/;
const ATTRIBUTE = /\s+([^\s=/>]+)\s*=\s*(?:"[^"]*"|'[^']*')/g;
const USES_XLINK = /\sxlink:[^\s=/>]+\s*=/;

/**
 * Elements that run code, load other content, navigate, or change an attribute over time (an animated `href`).
 * Matched lowercased: the page's HTML parse lowercases tag names in an svg, so `<SET>` is a `set` there.
 */
const DROPPED = new Set([
  "script",
  "foreignobject",
  "iframe",
  "object",
  "embed",
  "set",
  "animate",
  "animatemotion",
  "animatetransform",
  "discard",
  "meta",
  "link",
  "base",
]);
const CACHE_KEPT = 60;

/** The XML parser takes an attribute once; some engines write `xmlns` twice. */
function uniqueXmlns(svg: string): string {
  let seen = false;
  return svg.replace(/\sxmlns="[^"]*"/g, (match) => {
    if (seen) return "";
    seen = true;
    return match;
  });
}

/**
 * Mermaid writes `xlink:href` on its links without declaring the prefix, which the XML parser refuses. The root gets
 * the declaration (attribute order does not count in XML), never another element; a root that already has it, or a
 * start tag not read whole, is left as written.
 */
function declareXlink(svg: string): string {
  if (!USES_XLINK.test(svg)) return svg;
  const start = PROLOG.exec(svg)?.[0].length ?? 0;
  const tag = ROOT_TAG.exec(svg.slice(start));
  if (tag === null) return svg;
  const names = [...(tag[1] ?? "").matchAll(ATTRIBUTE)].map((attr) => attr[1]);
  if (names.includes("xmlns:xlink")) return svg;
  const at = start + "<svg".length;
  return `${svg.slice(0, at)} xmlns:xlink="${XLINK_NS}"${svg.slice(at)}`;
}

const nameOf = (node: Element | Attr): string => node.localName.toLowerCase();

/** A web, mail or in-page address, or a file of the project; never another scheme, a drive or a network path. */
function followable(href: string): boolean {
  if (CONTROL.test(href)) return false;
  const bare = href.replace(BLANKS, "");
  return bare !== "" && !NETWORK.test(bare) && (PAGE_LINK.test(bare) || isLocalHref(bare));
}

/** A `#id` that names, as written, a part of the drawing with no link and no other copy in it. */
function copiesPlainPart(doc: Document, href: string): boolean {
  const id = href.slice(1);
  const target = PLAIN_ID.test(id) ? doc.getElementById(id) : null;
  return (
    target !== null &&
    [target, ...target.querySelectorAll("*")].every((el) => !["a", "use"].includes(nameOf(el)))
  );
}

/**
 * A `<use>` draws a copy of its target in a tree of its own: a click on a copied link reaches the page as a click on
 * the `<use>`, outside any link a click guard looks for, while the copy still follows its address. One that copies a
 * link, another copy (a chain), or a part missing from the drawing goes.
 */
function removeLinkCopies(root: Element): void {
  const copies = [...root.querySelectorAll("*")].filter((el) => nameOf(el) === "use");
  const unsafe = copies.filter((use) =>
    [...use.attributes].some(
      (attr) => nameOf(attr) === "href" && !copiesPlainPart(root.ownerDocument, attr.value),
    ),
  );
  for (const use of unsafe) use.remove();
}

/**
 * The link with one plain `href` (when its address is followable) and nothing else that leads somewhere. The page's
 * link handling (View, Edit, export, replies) reads only a plain `href`; an address left in `xlink:href` would be
 * followed by the web view itself, in place of the app page, and a target (`xlink:show="new"` too) opens one beside.
 * The XML namespace of `href` is ignored: the page's HTML parse may read any of them as the address.
 */
function pageLink(link: Element, hrefs: readonly string[]): void {
  for (const attr of [...link.attributes]) {
    if (["href", "target", "show"].includes(nameOf(attr))) link.removeAttributeNode(attr);
  }
  const values = new Set(hrefs.map((href) => href.trim()));
  const [value] = values;
  // two different addresses: which one the page would follow is not known
  if (values.size === 1 && value !== undefined && followable(value))
    link.setAttribute("href", value);
}

/**
 * Comments and processing instructions go: the page parses the drawing as HTML, where `<?…` and `<!-->` end at the
 * first `>`, so markup written inside them would come alive.
 */
function removeHidden(root: Element): void {
  const walker = root.ownerDocument.createTreeWalker(
    root,
    NodeFilter.SHOW_COMMENT | NodeFilter.SHOW_PROCESSING_INSTRUCTION,
  );
  const hidden: Node[] = [];
  while (walker.nextNode()) hidden.push(walker.currentNode);
  for (const node of hidden) node.parentNode?.removeChild(node);
}

/**
 * Whether the page, parsing `markup` as HTML (`{@html}`), builds the tree the XML pass cleaned: one svg root, every
 * element in it SVG, as many as the XML tree held. Markup parses differently there (a `<p>` leaves the svg and takes
 * what follows with it; the content of a `<title>` is HTML). A template's content stays inert while it is checked.
 */
function readsAsCleaned(markup: string, elements: number): boolean {
  const template = document.createElement("template");
  template.innerHTML = markup;
  const [top, ...more] = [...template.content.children];
  if (top === undefined || more.length > 0) return false;
  if (top.localName !== "svg" || top.namespaceURI !== SVG_NS) return false;
  const inner = [...top.querySelectorAll("*")];
  return inner.length === elements && inner.every((el) => el.namespaceURI === SVG_NS);
}

/**
 * The drawing with scripts, foreign content, event handlers and references out of the drawing removed, its links as
 * plain page links; null when it is not SVG or would not stay one drawing on the page.
 */
export function sanitizeSvg(svg: string): string | null {
  const doc = new DOMParser().parseFromString(declareXlink(uniqueXmlns(svg)), "image/svg+xml");
  const root = doc.documentElement;
  // an unprefixed `svg` of the SVG namespace: the root the page's HTML parse makes, so both passes see one drawing
  if (root.tagName !== "svg" || root.namespaceURI !== SVG_NS || doc.querySelector("parsererror"))
    return null;
  removeHidden(root);
  for (const el of root.querySelectorAll("*")) {
    const name = nameOf(el);
    const loads = name === "style" && [...el.attributes].some((attr) => nameOf(attr) === "src");
    if (DROPPED.has(name) || loads) el.remove();
  }
  // the root too: a handler on the svg itself runs like any other
  for (const el of [root, ...root.querySelectorAll("*")]) {
    const hrefs: string[] = [];
    for (const attr of [...el.attributes]) {
      const name = nameOf(attr);
      if (name.startsWith("on")) el.removeAttributeNode(attr);
      else if (name === "href") hrefs.push(attr.value);
    }
    if (nameOf(el) === "a") pageLink(el, hrefs);
    // elsewhere a reference loads what it names: only a part of the drawing. A file beside the document never loaded
    // in the page anyway, and the engines bring their pictures as `data:`, which goes too.
    else if (!hrefs.every((href) => href.startsWith("#"))) {
      if (el === root) return null;
      el.remove();
    }
  }
  removeLinkCopies(root);
  const clean = new XMLSerializer().serializeToString(root);
  return readsAsCleaned(clean, root.querySelectorAll("*").length) ? clean : null;
}

/** The width a drawing was made at (from its viewBox), so the figure can size to it. */
export function viewBoxWidth(viewBox: string | null | undefined): number | null {
  const parts = (viewBox ?? "")
    .trim()
    .split(/[\s,]+/)
    .map(Number);
  const width = parts[2];
  return parts.length === 4 && width !== undefined && Number.isFinite(width) && width > 0
    ? width
    : null;
}

/** A forced theme ignores the page's light tone: its drawings never share a cache entry with the page's. */
export function paletteKey(theme: "light" | "dark", forced: boolean): string {
  return forced ? `${theme}:forced` : `${theme}:${document.documentElement.dataset["tone"] ?? ""}`;
}

/**
 * `read` gets the design tokens: the page's (theme + tone), or for a forced theme those of a probe element scoped to
 * that theme alone (the tokens scope each theme by `data-theme`; the tones apply to the root only).
 */
export function withTokens<T>(
  theme: "light" | "dark",
  forced: boolean,
  read: (token: (name: string) => string) => T,
): T {
  const reader = (element: Element) => {
    const css = getComputedStyle(element);
    return (name: string): string => css.getPropertyValue(name).trim();
  };
  if (!forced) return read(reader(document.documentElement));
  const probe = document.createElement("div");
  probe.hidden = true;
  probe.dataset["theme"] = theme;
  document.body.append(probe);
  try {
    // read before the probe leaves: a detached element has no computed tokens
    return read(reader(probe));
  } finally {
    probe.remove();
  }
}

class DiagramQueue {
  #tail: Promise<unknown> = Promise.resolve();
  #cache = new Map<string, string>();

  /**
   * The drawing for `key`: cached, else made by `job` after the ones queued before it. `null` | a failure: it cannot
   * be drawn (never cached: the next ask tries again).
   */
  draw<R extends string | DrawFailure | null>(
    key: string,
    job: () => Promise<R>,
  ): Promise<R | string | null> {
    const cached = this.#cache.get(key);
    if (cached !== undefined) return Promise.resolve(cached);
    const run = this.#tail.then(async () => {
      try {
        const svg = await job();
        if (typeof svg !== "string") return svg;
        if (this.#cache.size >= CACHE_KEPT) this.#cache.clear();
        this.#cache.set(key, svg);
        return svg;
      } catch {
        return null;
      }
    });
    this.#tail = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }

  /** Forgets every drawing (tests). */
  clear(): void {
    this.#cache.clear();
  }
}

export const diagrams = new DiagramQueue();
