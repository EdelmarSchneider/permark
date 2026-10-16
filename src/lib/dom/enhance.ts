// Post-processing of the backend's HTML after it is injected: code blocks (copy button, language label, highlighting),
// tables, heading anchors, alerts, task lists, KaTeX, Mermaid and relative images.
// Structural changes are synchronous and keep one element per top-level block, so the block diff
// can address blocks by index; the heavy libraries load lazily and fill in afterwards.

import type { CodeOutcome } from "../code/highlighter";
import { resolveRelative } from "../paths";
import { viewBoxWidth } from "./breakout";
import { decorateCodeRefs, type CodeRefsContext } from "./codeRefsDom";

export type EnhanceLabels = {
  copyCode: string;
  /** The corner label of a block whose language was guessed ("auto: Python"). */
  codeAuto: (name: string) => string;
  /** What the guessed-language label says when it is hovered. */
  codeAutoHint: string;
  anchorLabel: (name: string) => string;
  diagramPending: string;
  diagramFailed: string;
  alertTitles: Record<string, string>;
};

export type EnhanceContext = {
  docPath: string;
  theme: "light" | "dark";
  labels: EnhanceLabels;
  /** Resolves a vault-relative image path to a URL the webview can load. */
  resolveImage: (path: string) => Promise<string>;
  /** Makes the code references (`src/main.rs:42`) links. Only the reading view sets it: nothing handles their clicks elsewhere. */
  codeRefs?: CodeRefsContext;
};

const SVG_ATTRS =
  'viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"';

// Lucide icons used inside injected markup
export const ICON_COPY = `<svg ${SVG_ATTRS}><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>`;
export const ICON_CHECK = `<svg ${SVG_ATTRS}><path d="M20 6 9 17l-5-5"/></svg>`;
const ALERT_ICONS: Record<string, string> = {
  note: `<svg ${SVG_ATTRS}><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`,
  tip: `<svg ${SVG_ATTRS}><path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5"/><path d="M9 18h6"/><path d="M10 22h4"/></svg>`,
  important: `<svg ${SVG_ATTRS}><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/><path d="M12 7v2"/><path d="M12 13h.01"/></svg>`,
  warning: `<svg ${SVG_ATTRS}><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>`,
  caution: `<svg ${SVG_ATTRS}><path d="M12 16h.01"/><path d="M12 8v4"/><path d="M15.312 2a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586l-4.688-4.688A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2z"/></svg>`,
};

function copySourcepos(from: Element, to: Element): void {
  const pos = from.getAttribute("data-sourcepos");
  if (pos) to.setAttribute("data-sourcepos", pos);
}

// ---- before insertion: images -----------------------------------------------------------------

/**
 * Detaches relative image sources from a not-yet-inserted fragment, so the webview does not request
 * them from the app origin. They are resolved to asset URLs after insertion.
 */
export function stashRelativeImages(root: ParentNode, docPath: string): void {
  for (const img of Array.from(root.querySelectorAll("img"))) {
    const src = img.getAttribute("src");
    if (src === null) continue;
    const resolved = resolveRelative(docPath, src);
    if (resolved === null) continue;
    img.setAttribute("data-tm-path", resolved);
    img.removeAttribute("src");
  }
}

// ---- after insertion ----------------------------------------------------------------------------

export type Enhanced = { els: HTMLElement[]; tasks: Promise<void>[] };

export function enhanceBlocks(els: HTMLElement[], ctx: EnhanceContext): Enhanced {
  const tasks: Promise<void>[] = [];
  const out = els.map((el) => {
    let top = el;
    if (el.tagName === "PRE") top = wrapCode(el, ctx, tasks);
    else if (el.tagName === "TABLE") top = wrapTable(el);
    enhanceInside(top, ctx, tasks);
    return top;
  });
  return { els: out, tasks };
}

function enhanceInside(root: HTMLElement, ctx: EnhanceContext, tasks: Promise<void>[]): void {
  for (const pre of Array.from(root.querySelectorAll("pre"))) {
    if (pre.closest(".code-block, .diagram")) continue;
    wrapCode(pre, ctx, tasks);
  }
  for (const table of Array.from(root.querySelectorAll("table"))) {
    if (!table.parentElement?.classList.contains("table-wrap")) wrapTable(table);
  }
  root
    .querySelectorAll<HTMLElement>("h1, h2, h3, h4, h5, h6")
    .forEach((heading) => ensureAnchor(heading, ctx));
  if (root.matches("h1, h2, h3, h4, h5, h6")) ensureAnchor(root, ctx);
  decorateAlerts(root, ctx);
  decorateTaskLists(root);
  if (root.tagName === "P" && isImageOnly(root)) root.classList.add("tm-wide");
  root.querySelectorAll<HTMLElement>("[data-math-style]").forEach((el) => {
    tasks.push(renderMath(el));
  });
  root.querySelectorAll<HTMLImageElement>("img[data-tm-path]").forEach((img) => {
    tasks.push(loadImage(img, ctx));
  });
  if (ctx.codeRefs) decorateCodeRefs(root, ctx.codeRefs);
}

function wrapTable(table: HTMLElement): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "table-wrap";
  copySourcepos(table, wrap);
  table.replaceWith(wrap);
  wrap.appendChild(table);
  return wrap;
}

/** What the fence's info string said, as the backend writes it into the class (`language-rust` for ```` ```rust title="x" ````), or null. */
function infoOf(code: Element | null): string | null {
  const token = (code?.getAttribute("class") ?? "")
    .split(/\s+/)
    .find((name) => name.startsWith("language-"));
  return token === undefined ? null : token.slice("language-".length);
}

function wrapCode(pre: HTMLElement, ctx: EnhanceContext, tasks: Promise<void>[]): HTMLElement {
  const code = pre.querySelector("code");
  const info = infoOf(code);
  const source = (code ?? pre).textContent ?? "";

  if (info !== null && info.toLowerCase() === "mermaid") {
    const figure = document.createElement("figure");
    figure.className = "diagram is-pending";
    figure.dataset.source = source;
    figure.textContent = ctx.labels.diagramPending;
    copySourcepos(pre, figure);
    pre.replaceWith(figure);
    tasks.push(renderDiagram(figure, ctx));
    return figure;
  }

  const wrap = document.createElement("div");
  wrap.className = "code-block";
  copySourcepos(pre, wrap);
  pre.replaceWith(wrap);
  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "icon-btn icon-btn--sm code-block__copy";
  copy.setAttribute("aria-label", ctx.labels.copyCode);
  copy.title = ctx.labels.copyCode;
  copy.innerHTML = ICON_COPY;
  wrap.appendChild(copy);
  wrap.appendChild(pre);
  if (code) tasks.push(decorateCode(code, wrap, info ?? "", source, ctx));
  return wrap;
}

function ensureAnchor(heading: HTMLElement, ctx: EnhanceContext): void {
  const name = (heading.textContent ?? "").replace(/^#/, "").trim();
  let anchor = heading.querySelector<HTMLAnchorElement>("a.anchor");
  if (!anchor) {
    const id = heading.id;
    if (!id) return;
    anchor = document.createElement("a");
    anchor.className = "anchor";
    anchor.setAttribute("href", `#${id}`);
    heading.prepend(anchor);
  }
  anchor.textContent = "#";
  anchor.removeAttribute("aria-hidden");
  anchor.setAttribute("aria-label", ctx.labels.anchorLabel(name));
}

function decorateAlerts(root: HTMLElement, ctx: EnhanceContext): void {
  const alerts = root.matches(".markdown-alert")
    ? [root]
    : Array.from(root.querySelectorAll<HTMLElement>(".markdown-alert"));
  for (const alert of alerts) {
    const title = alert.querySelector<HTMLElement>(".markdown-alert-title");
    if (!title || title.querySelector("svg")) continue;
    const kind = /markdown-alert-(\w+)/.exec(alert.className)?.[1] ?? "note";
    const text = ctx.labels.alertTitles[kind];
    if (text) title.textContent = text;
    title.insertAdjacentHTML("afterbegin", ALERT_ICONS[kind] ?? ALERT_ICONS.note);
  }
}

function decorateTaskLists(root: HTMLElement): void {
  root.querySelectorAll<HTMLInputElement>('li > input[type="checkbox"]').forEach((box) => {
    const item = box.parentElement;
    if (!item) return;
    item.classList.add("task-list-item");
    item.classList.toggle("is-done", box.checked);
    item.parentElement?.classList.add("contains-task-list");
  });
}

// ---- blocks that may outgrow the text measure ----------------------------------------------------

/** Paragraph content that counts: elements, and text that is not blank. */
function meaningfulNodes(parent: Element): ChildNode[] {
  return Array.from(parent.childNodes).filter(
    (node) =>
      node.nodeType === Node.ELEMENT_NODE ||
      (node.nodeType === Node.TEXT_NODE && (node.textContent ?? "").trim() !== ""),
  );
}

/** A paragraph that is one image, possibly inside a link. Its width is the image's (see `.tm-wide`). */
function isImageOnly(paragraph: HTMLElement): boolean {
  const [only, ...rest] = meaningfulNodes(paragraph);
  if (!(only instanceof HTMLElement) || rest.length > 0) return false;
  if (only.tagName === "IMG") return true;
  if (only.tagName !== "A") return false;
  const [inner, ...more] = meaningfulNodes(only);
  return inner instanceof HTMLElement && inner.tagName === "IMG" && more.length === 0;
}

/** Display math is the only thing in its paragraph: the paragraph may then be as wide as the formula. */
function markDisplayMath(formula: HTMLElement): void {
  const paragraph = formula.parentElement;
  if (!paragraph || paragraph.tagName !== "P") return;
  if (!paragraph.parentElement?.classList.contains("md-body")) return;
  const [only, ...rest] = meaningfulNodes(paragraph);
  if (only === formula && rest.length === 0) paragraph.classList.add("tm-wide");
}

// ---- lazy libraries ---------------------------------------------------------------------------

/** How the blocks seen lately turned out, so that a document drawn again (a live update) shows them at once, not after a flash of plain text. */
const codeOutcomes = new Map<string, CodeOutcome>();
const CODE_OUTCOMES_KEPT = 200;

function showCode(
  code: Element,
  wrap: HTMLElement,
  outcome: CodeOutcome,
  source: string,
  ctx: EnhanceContext,
): void {
  // the block was drawn again with other text meanwhile: this answer is not for it
  if ((code.textContent ?? "") !== source) return;
  wrap.querySelector(".code-block__lang")?.remove();
  if (outcome.html !== null) {
    code.innerHTML = outcome.html;
    code.classList.add("hljs");
  }
  const { label } = outcome;
  if (label) {
    const element = document.createElement("span");
    element.className = "code-block__lang";
    element.textContent = label.auto ? ctx.labels.codeAuto(label.name) : label.name;
    if (label.auto) element.title = ctx.labels.codeAutoHint;
    wrap.insertBefore(element, wrap.firstChild);
  }
}

/**
 * Highlights a fenced block and puts the name of its language in its corner: the grammar of the language it names, or the best guess
 * for one that names none (see `highlightFence`). The libraries load lazily; the block is plain text until they have.
 */
async function decorateCode(
  code: Element,
  wrap: HTMLElement,
  info: string,
  source: string,
  ctx: EnhanceContext,
): Promise<void> {
  const key = `${info}\u0000${source}`;
  const known = codeOutcomes.get(key);
  if (known) {
    showCode(code, wrap, known, source, ctx);
    return;
  }
  try {
    const { highlightFence } = await import("../code/highlighter");
    const outcome = await highlightFence(source, info);
    if (codeOutcomes.size >= CODE_OUTCOMES_KEPT) codeOutcomes.clear();
    codeOutcomes.set(key, outcome);
    showCode(code, wrap, outcome, source, ctx);
  } catch {
    // keep the plain text
  }
}

async function renderMath(el: HTMLElement): Promise<void> {
  const source = el.textContent ?? "";
  try {
    const [{ default: katex }] = await Promise.all([
      import("katex"),
      import("katex/dist/katex.min.css"),
    ]);
    const display = el.dataset.mathStyle === "display";
    katex.render(source, el, { displayMode: display, throwOnError: true, output: "htmlAndMathml" });
    if (display) markDisplayMath(el);
  } catch (error) {
    el.textContent = source;
    el.classList.add("tm-math-error");
    el.title = error instanceof Error ? error.message : String(error);
  }
}

const imageUrls = new Map<string, Promise<string>>();

async function loadImage(img: HTMLImageElement, ctx: EnhanceContext): Promise<void> {
  const path = img.getAttribute("data-tm-path");
  if (!path) return;
  img.addEventListener("error", () => img.classList.add("is-broken"), { once: true });
  try {
    let url = imageUrls.get(path);
    if (!url) {
      url = ctx.resolveImage(path);
      imageUrls.set(path, url);
      url.catch(() => imageUrls.delete(path));
    }
    img.src = await url;
  } catch {
    img.classList.add("is-broken");
  }
}

// ---- Mermaid ------------------------------------------------------------------------------------

type Mermaid = typeof import("mermaid").default;
let mermaidModule: Mermaid | null = null;
let mermaidTheme: string | null = null;
let mermaidQueue: Promise<unknown> = Promise.resolve();
let mermaidCounter = 0;
const diagramCache = new Map<string, string>();

async function loadMermaid(theme: "light" | "dark"): Promise<Mermaid> {
  if (!mermaidModule) mermaidModule = (await import("mermaid")).default;
  if (mermaidTheme !== theme) {
    // the diagram palette comes from the design tokens (solid colors only; Mermaid cannot parse rgb alpha)
    const css = getComputedStyle(document.documentElement);
    const token = (name: string): string => css.getPropertyValue(name).trim();
    mermaidModule.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: "base",
      fontFamily: '"Inter Variable", Inter, system-ui, sans-serif',
      themeVariables: {
        darkMode: theme === "dark",
        background: token("--bg"),
        primaryColor: token("--surface"),
        primaryBorderColor: token("--border-strong"),
        primaryTextColor: token("--text"),
        secondaryColor: token("--surface-2"),
        tertiaryColor: token("--bg"),
        lineColor: token("--muted"),
        // edge labels sit on the page, not on a derived shade (the base theme computes pure black in dark mode)
        edgeLabelBackground: token("--bg"),
        textColor: token("--text"),
        noteBkgColor: token("--surface-2"),
        noteTextColor: token("--text"),
      },
    });
    mermaidTheme = theme;
  }
  return mermaidModule;
}

/** Renders (or re-renders, after a theme change) the diagram held in `figure.dataset.source`. */
export function renderDiagram(figure: HTMLElement, ctx: EnhanceContext): Promise<void> {
  const source = figure.dataset.source ?? "";
  const key = `${ctx.theme}\u0000${source}`;
  const cached = diagramCache.get(key);
  if (cached !== undefined) {
    showDiagram(figure, cached);
    return Promise.resolve();
  }
  const job = mermaidQueue.then(async () => {
    const id = `tm-mermaid-${++mermaidCounter}`;
    try {
      const mermaid = await loadMermaid(ctx.theme);
      const { svg } = await mermaid.render(id, source);
      if (diagramCache.size > 60) diagramCache.clear();
      diagramCache.set(key, svg);
      showDiagram(figure, svg);
    } catch {
      showDiagramError(figure, source, ctx);
      // Mermaid leaves its error graphic behind in the page when a render fails
      document.getElementById(id)?.remove();
    } finally {
      document.getElementById(`d${id}`)?.remove();
    }
  });
  mermaidQueue = job.catch(() => undefined);
  return job;
}

function showDiagram(figure: HTMLElement, svg: string): void {
  figure.className = "diagram";
  figure.innerHTML = svg;
  // Mermaid draws `width="100%"`, which has no size of its own: give the SVG the width it was drawn at, so the
  // figure can size to the diagram (and outgrow the text measure when the diagram is wide)
  const drawing = figure.querySelector("svg");
  const width = viewBoxWidth(drawing?.getAttribute("viewBox"));
  if (drawing && width !== null) {
    drawing.style.width = `${width}px`;
    drawing.style.maxWidth = "100%";
  }
}

function showDiagramError(figure: HTMLElement, source: string, ctx: EnhanceContext): void {
  figure.className = "diagram-error";
  figure.replaceChildren();
  const block = document.createElement("div");
  block.className = "code-block";
  const pre = document.createElement("pre");
  const code = document.createElement("code");
  code.textContent = source;
  pre.appendChild(code);
  block.appendChild(pre);
  const message = document.createElement("p");
  message.className = "diagram-error__msg";
  message.textContent = ctx.labels.diagramFailed;
  figure.append(block, message);
}
