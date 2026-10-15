// Turns the code references of a rendered document into links: inline code that is a path (`src/main.rs:42`) and plain text
// with a path and a line. What counts as a reference, and which file it names, is decided by `../codeRefs.ts`; this module
// only walks the DOM. It never touches code blocks or existing links, and everything it adds can be taken off again
// (`clearCodeRefs`), so the references are looked up anew when the project's files change.

import { findTextRefs, parseInlineRef, type CodeRef } from "../codeRefs";

export type CodeRefsContext = {
  /** The project files a written path can mean: none (not a link), one, or several (a menu on click). */
  resolve: (written: string) => string[];
  /** The tooltip of a reference that names one file. */
  title: (path: string, line: number | null) => string;
  /** The tooltip of a reference that could be any of `count` files. */
  titleMany: (count: number) => string;
};

/** What a reference element stands for. */
export type RefTarget = {
  /** The file, when the reference names exactly one. */
  path: string | null;
  /** The files it could mean, when it names several (otherwise empty). */
  candidates: string[];
  line: number | null;
  col: number | null;
};

const CLASS = "code-ref";
/** Where a reference is never made: code blocks, links, buttons, math, diagrams, and what is already a reference. */
const SKIP =
  "pre, a, button, kbd, svg, script, style, textarea, .code-ref, .katex, [data-math-style], .diagram, .diagram-error, .tm-removed, .anchor";

/** The attributes `mark` sets, besides the class. */
const MARK_ATTRIBUTES = [
  "role",
  "tabindex",
  "title",
  "data-ref-path",
  "data-ref-candidates",
  "data-ref-line",
  "data-ref-col",
];

function mark(el: HTMLElement, ref: CodeRef, paths: string[], ctx: CodeRefsContext): void {
  el.classList.add(CLASS);
  el.setAttribute("role", "link");
  el.tabIndex = 0;
  if (ref.line !== null) el.dataset.refLine = String(ref.line);
  if (ref.col !== null) el.dataset.refCol = String(ref.col);
  if (paths.length === 1) {
    el.dataset.refPath = paths[0];
    el.title = ctx.title(paths[0], ref.line);
  } else {
    el.dataset.refCandidates = JSON.stringify(paths);
    el.title = ctx.titleMany(paths.length);
  }
}

/** Marks the references under `root` (a block, or the whole document). */
export function decorateCodeRefs(root: ParentNode, ctx: CodeRefsContext): void {
  for (const code of Array.from(root.querySelectorAll("code"))) {
    if (code.closest(SKIP) || code.children.length > 0) continue;
    const ref = parseInlineRef(code.textContent ?? "");
    const paths = ref ? ctx.resolve(ref.path) : [];
    if (ref && paths.length > 0) mark(code, ref, paths, ctx);
  }

  const walker = document.createTreeWalker(root as Node, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => {
      const parent = node.parentElement;
      if (!parent || parent.closest(SKIP) || parent.closest("code"))
        return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  const texts: Text[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) texts.push(node as Text);
  for (const text of texts) wrapTextRefs(text, ctx);
}

/** Wraps the references of one text node in spans, leaving the rest of the text as it is. */
function wrapTextRefs(node: Text, ctx: CodeRefsContext): void {
  const text = node.data;
  const pieces: (string | HTMLElement)[] = [];
  let last = 0;
  for (const { start, end, ref } of findTextRefs(text)) {
    const paths = ctx.resolve(ref.path);
    if (paths.length === 0) continue;
    const span = document.createElement("span");
    span.textContent = text.slice(start, end);
    mark(span, ref, paths, ctx);
    pieces.push(text.slice(last, start), span);
    last = end;
  }
  if (pieces.length === 0) return;
  pieces.push(text.slice(last));
  node.replaceWith(...pieces.filter((piece) => piece !== ""));
}

/** Takes every reference mark off `root` again. */
export function clearCodeRefs(root: ParentNode): void {
  const parents = new Set<Node>();
  for (const el of Array.from(root.querySelectorAll<HTMLElement>(`.${CLASS}`))) {
    if (el.tagName === "CODE") {
      el.classList.remove(CLASS);
      if (el.classList.length === 0) el.removeAttribute("class");
      for (const name of MARK_ATTRIBUTES) el.removeAttribute(name);
    } else {
      if (el.parentNode) parents.add(el.parentNode);
      el.replaceWith(el.textContent ?? "");
    }
  }
  // the pieces of text that were split around the spans are one text again
  for (const parent of parents) parent.normalize();
}

/** What a marked element refers to, or null when it is not a reference. */
export function refTargetOf(el: Element | null): RefTarget | null {
  const ref = el?.closest<HTMLElement>(`.${CLASS}`);
  if (!ref) return null;
  const number = (value: string | undefined): number | null => {
    const n = Number(value);
    return value !== undefined && Number.isInteger(n) && n >= 1 ? n : null;
  };
  let candidates: string[] = [];
  try {
    const parsed: unknown = JSON.parse(ref.dataset.refCandidates ?? "[]");
    if (Array.isArray(parsed))
      candidates = parsed.filter((p): p is string => typeof p === "string");
  } catch {
    candidates = [];
  }
  const path = ref.dataset.refPath ?? null;
  if (path === null && candidates.length === 0) return null;
  return { path, candidates, line: number(ref.dataset.refLine), col: number(ref.dataset.refCol) };
}
