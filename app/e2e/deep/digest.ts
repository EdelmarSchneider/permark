// Text goldens of a rendered page: the renderer's blocks (from the rendered corpus) and a digest of the reading view
// after everything is drawn. Both leave out what changes without the page changing: block ids, generated ids, the
// compiler's style-scope classes, geometry (fonts differ per OS; pixel baselines hold the looks).

/** A rendered block as the backend sends it (`Block`). */
type Block = { kind: string; lineStart: number; lineEnd: number; html: string | null };

/** The parts of a rendered view the block golden reads (`DocumentView`). */
export type RenderedView = {
  path: string;
  format: string;
  body: {
    kind: string;
    blocks?: Block[];
    outline?: { level: number; label: string; line: number }[];
    frontMatter?: unknown;
  };
};

const START_TAG = /<([a-z][a-z0-9-]*)((?:\s+[^\s"'>/=]+(?:="[^"]*")?)*)\s*(\/?)>/g;
const ATTRIBUTE = /[^\s"'>/=]+(?:="[^"]*")?/g;

/** `html` with each start tag's attributes in name order: the sanitizer writes the attributes it sets (a player's
 *  `controls` and `preload`) in an order that changes from run to run. */
export function sortedAttributes(html: string): string {
  return html.replace(START_TAG, (_tag, name: string, attrs: string, slash: string) => {
    const sorted = (attrs.match(ATTRIBUTE) ?? []).sort();
    return `<${name}${sorted.map((a) => ` ${a}`).join("")}${slash}>`;
  });
}

/** The block golden: front matter, outline and every block's kind, lines and HTML (its block id left out). */
export function blockGolden(view: RenderedView): string {
  const lines = [`# ${view.path}`, `format: ${view.format}`];
  const body = view.body;
  if (body.frontMatter !== undefined && body.frontMatter !== null) {
    lines.push(`front matter: ${JSON.stringify(body.frontMatter)}`);
  }
  for (const item of body.outline ?? []) {
    lines.push(`outline: ${"  ".repeat(item.level - 1)}${item.label} (line ${item.line})`);
  }
  for (const block of body.blocks ?? []) {
    lines.push(`--- ${block.kind} ${block.lineStart}-${block.lineEnd}`);
    lines.push(sortedAttributes((block.html ?? "").replace(/ data-b="[^"]*"/g, "")));
  }
  return `${lines.join("\n")}\n`;
}

/** File-safe name of a corpus page: `demo/README.md` -> `demo__README`. */
export function slugOf(path: string): string {
  return path
    .replace(/\.(md|markdown)$/i, "")
    .replace(/[\\/]/g, "__")
    .replace(/[^A-Za-z0-9_.-]/g, "-");
}

/**
 * The digest of the element at `selector` (runs in the page): one line per element (tag, kept classes, kept
 * attributes) and text node, indented by depth. A picture drawn as SVG is its kind, whether it has a size and its
 * text labels; a formula is its TeX source; highlighted code is its text and the token kinds in it.
 */
export function domDigest(arg: { selector: string; origin: string }): string | null {
  const root = document.querySelector(arg.selector);
  if (root === null) return null;
  const out: string[] = [];
  const KEPT_ATTRS = [
    "href",
    "src",
    "alt",
    "title",
    "role",
    "aria-label",
    "id",
    "name",
    "data-math-style",
    "data-tag",
    "open",
    "checked",
    "disabled",
    "colspan",
    "rowspan",
    "align",
    "dir",
    "lang",
    "controls",
  ];
  const flat = (text: string, max: number): string => {
    const one = text.replace(/\s+/g, " ").trim();
    return one.length > max ? `${one.slice(0, max)}…` : one;
  };
  // generated ids (diagram engines, counters) carry long digit runs
  const stable = (value: string): string =>
    value
      .replaceAll(arg.origin, "")
      .replace(/\d{4,}/g, "#")
      .replace(/[0-9a-f]{16,}/g, "#");
  // states of the moment: deferred work not done yet; table widths measured from this screen's layout and fonts
  const PASSING = new Set(["is-pending", "pm-screen-table", "pm-fit-table", "is-natural"]);
  const classesOf = (el: Element): string =>
    [...el.classList]
      .filter((c) => !c.startsWith("svelte-") && !PASSING.has(c))
      .map((c) => `.${c}`)
      .join("");
  const attrsOf = (el: Element): string =>
    KEPT_ATTRS.flatMap((name) => {
      const value = el.getAttribute(name);
      return value === null ? [] : [value === "" ? name : `${name}="${flat(stable(value), 80)}"`];
    }).join(" ");
  const svgLine = (svg: SVGSVGElement): string => {
    if (svg.classList.contains("lucide")) {
      const icon = [...svg.classList].find((c) => c.startsWith("lucide-") && c !== "lucide-icon");
      return `icon ${icon ?? "lucide"}`;
    }
    const kind = svg.getAttribute("aria-roledescription") ?? (classesOf(svg) || "svg");
    const box = svg.viewBox.baseVal;
    const rect = svg.getBoundingClientRect();
    const sized = (box !== null && box.width > 0 && box.height > 0) || rect.width > 0;
    const labels: string[] = [];
    for (const t of svg.querySelectorAll("text, foreignObject")) {
      if (t.parentElement?.closest("text, foreignObject") !== null) continue;
      const label = flat(t.textContent ?? "", 60);
      if (label !== "" && labels.at(-1) !== label) labels.push(label);
    }
    return `svg ${kind} ${sized ? "sized" : "EMPTY"}${labels.length > 0 ? ` labels: ${labels.join(" | ")}` : ""}`;
  };
  const walk = (node: Node, depth: number): void => {
    const pad = "  ".repeat(depth);
    if (node.nodeType === Node.TEXT_NODE) {
      const text = flat(node.textContent ?? "", 160);
      if (text !== "") out.push(`${pad}"${text}"`);
      return;
    }
    if (!(node instanceof Element)) return;
    if (node instanceof SVGSVGElement) {
      out.push(pad + svgLine(node));
      return;
    }
    const tag = node.tagName.toLowerCase();
    // measured column widths (see the classes above), not the document's
    if (tag === "colgroup" && node.parentElement?.classList.contains("pm-screen-table")) return;
    if (node.classList.contains("katex")) {
      const tex = node.querySelector('annotation[encoding="application/x-tex"]')?.textContent ?? "";
      const error = node.querySelector(".katex-error") !== null ? " ERROR" : "";
      out.push(`${pad}katex${error} tex="${flat(tex, 160)}"`);
      return;
    }
    if (node.classList.contains("katex-error")) {
      out.push(`${pad}katex-error "${flat(node.textContent ?? "", 160)}"`);
      return;
    }
    const attrs = attrsOf(node);
    if (tag === "code" && node.querySelector("[class*='hljs-']") !== null) {
      const kinds = new Set<string>();
      for (const span of node.querySelectorAll("[class*='hljs-']")) {
        for (const c of span.classList) if (c.startsWith("hljs-")) kinds.add(c);
      }
      const text = (node.textContent ?? "").replace(/\n/g, "⏎");
      out.push(`${pad}code${classesOf(node)}${attrs !== "" ? ` ${attrs}` : ""}`);
      out.push(`${pad}  tokens: ${[...kinds].sort().join(" ")}`);
      out.push(`${pad}  "${text.length > 300 ? `${text.slice(0, 300)}…` : text}"`);
      return;
    }
    out.push(`${pad}${tag}${classesOf(node)}${attrs !== "" ? ` ${attrs}` : ""}`);
    for (const child of node.childNodes) walk(child, depth + 1);
  };
  walk(root, 0);
  return `${out.join("\n")}\n`;
}
