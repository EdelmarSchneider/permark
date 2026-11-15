// The styles of an export page: the reading view's own (tokens, Markdown, code colors, formulas, the scoped styles
// of the embeds on the page), the fonts it uses as data URIs, the export page rules, and the page rule of the paper.

import type { PageBox } from "../../api/generated/PageBox";
import markdown from "../../core/doc/markdown.css?inline";
import fonts from "../../core/styles/fonts.css?inline";
import tokens from "../../core/styles/tokens.css?inline";
import highlight from "../highlight/tokens.css?inline";
import { fontStem, inlineFontUrls } from "./assets";
import paper from "./paper.css?inline";

type FontFiles = Record<string, () => Promise<string>>;

// loaded only when an export needs them (each file arrives as a data URI)
const APP_FONTS: FontFiles = import.meta.glob<string>(
  [
    "../../../node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2",
    "../../../node_modules/@fontsource-variable/inter/files/inter-latin-ext-wght-normal.woff2",
    "../../../node_modules/@fontsource-variable/jetbrains-mono/files/jetbrains-mono-latin-wght-normal.woff2",
    "../../../node_modules/@fontsource-variable/jetbrains-mono/files/jetbrains-mono-latin-ext-wght-normal.woff2",
  ],
  { query: "?inline", import: "default" },
);
const KATEX_FONTS: FontFiles = import.meta.glob<string>(
  "../../../node_modules/katex/dist/fonts/*.woff2",
  {
    query: "?inline",
    import: "default",
  },
);

/** Characters only the latin-ext font files hold (the latin files cover Western European text). */
const LATIN_EXT = /[Ā-˿ᴀ-ỿⱠ-Ɀ꜠-ꟿ]/;

export type LookOptions = {
  target: "paper" | "html";
  page?: PageBox;
  pageNumbers: boolean;
  nativeMargins?: boolean;
};

const mm = (value: number): string => `${Number(value.toFixed(2))}mm`;

/**
 * The paper's page rule (size and margins of the page box), with "n / total" at the bottom when asked. Where the web
 * view puts the margins itself (`nativeMargins`) the rule asks for none: CSS margins would add to them, and margin
 * boxes need a CSS margin. The printable height stays: the native margins take the same room.
 */
export function pageRule(page: PageBox, numbers: boolean, nativeMargins = false): string {
  const count =
    numbers && !nativeMargins
      ? ' @bottom-center { content: counter(page) " / " counter(pages); font: 9pt var(--font-reading); color: var(--muted); }'
      : "";
  const margin = nativeMargins ? "0" : mm(page.marginMm);
  return [
    `@page { size: ${mm(page.widthMm)} ${mm(page.heightMm)}; margin: ${margin};${count} }`,
    `:root { --page-h: ${mm(page.heightMm - 2 * page.marginMm)}; }`,
  ].join("\n");
}

/** The dark theme's tokens on the root when the reader prefers dark (the app scopes them by `data-theme`). */
export function darkPreference(css: string): string {
  const body = /\[data-theme="dark"\]\s*\{([^}]*)\}/.exec(css)?.[1];
  return body === undefined ? "" : `@media (prefers-color-scheme: dark) {\n:root {${body}}\n}`;
}

/** Pseudo-elements never match an element: the rule is about the element before them. */
const PSEUDO_ELEMENT =
  /::?(?:before|after|marker|placeholder|selection|backdrop|first-line|first-letter)\b/g;

function matches(root: Element, selector: string): boolean {
  const plain = selector.replace(PSEUDO_ELEMENT, "");
  try {
    return root.matches(plain) || root.querySelector(plain) !== null;
  } catch {
    return false;
  }
}

function scopedRules(rules: CSSRuleList, root: Element): string[] {
  const kept: string[] = [];
  for (const rule of Array.from(rules)) {
    if (rule instanceof CSSStyleRule) {
      if (rule.selectorText.includes("svelte-") && matches(root, rule.selectorText)) {
        kept.push(rule.cssText);
      }
    } else if (rule instanceof CSSGroupingRule) {
      const inner = scopedRules(rule.cssRules, root);
      const head = rule.cssText.slice(0, rule.cssText.indexOf("{")).trim();
      if (inner.length > 0 && head !== "") kept.push(`${head} {\n${inner.join("\n")}\n}`);
    }
  }
  return kept;
}

/** The scoped styles of the components drawn in `root` (code blocks, diagrams, formulas), as the window holds them. */
export function componentRules(root: Element): string {
  const kept: string[] = [];
  for (const sheet of Array.from(document.styleSheets)) {
    let rules: CSSRuleList;
    try {
      rules = sheet.cssRules;
    } catch {
      continue;
    }
    kept.push(...scopedRules(rules, root));
  }
  return kept.join("\n");
}

/** A font file's name without folder and extension (its name in the source tree). */
const stemOf = (file: string): string =>
  file
    .split("/")
    .pop()
    ?.replace(/\.woff2$/, "") ?? file;

const APP_STEMS = Object.keys(APP_FONTS).map(stemOf);

async function loadFonts(
  files: FontFiles,
  wanted: (stem: string) => boolean,
): Promise<Map<string, string>> {
  const loaded = await Promise.all(
    Object.entries(files)
      .filter(([file]) => wanted(stemOf(file)))
      .map(async ([file, load]): Promise<[string, string]> => [stemOf(file), await load()]),
  );
  return new Map(loaded);
}

/** The `@font-face` rules of `css` whose source file is wanted (the others go: their files are not embedded). */
function faces(css: string, stems: readonly string[], wanted: (stem: string) => boolean): string {
  return css.replace(/@font-face\s*\{[^}]*\}/g, (face) => {
    const stem = fontStem(/url\(\s*["']?([^"')]+)/.exec(face)?.[1] ?? "", stems);
    return stem !== null && wanted(stem) ? face : "";
  });
}

/** The app fonts the text needs: Inter always, JetBrains Mono for code, the latin-ext files for such characters. */
async function appFonts(root: Element): Promise<string> {
  const code = root.querySelector("code, pre, kbd, samp") !== null;
  const ext = LATIN_EXT.test(root.textContent ?? "");
  const wanted = (stem: string): boolean =>
    (stem.startsWith("inter-") || (code && stem.startsWith("jetbrains-mono-"))) &&
    (ext || !stem.includes("-latin-ext-"));
  return inlineFontUrls(faces(fonts, APP_STEMS, wanted), await loadFonts(APP_FONTS, wanted));
}

/** KaTeX's styles with its fonts inside, when the page holds a formula. */
async function mathLook(root: Element): Promise<string> {
  if (root.querySelector(".katex") === null) return "";
  const [css, files] = await Promise.all([
    import("katex/dist/katex.min.css?inline").then((module) => module.default),
    loadFonts(KATEX_FONTS, () => true),
  ]);
  return inlineFontUrls(css, files);
}

/** Every style of the export page of `root` (the cleaned copy of the drawn document). */
export async function exportCss(root: Element, options: LookOptions): Promise<string> {
  const [app, math] = await Promise.all([appFonts(root), mathLook(root)]);
  const parts = [app, tokens];
  if (options.target === "html") parts.push(darkPreference(tokens));
  parts.push(markdown, highlight, math, componentRules(root), paper);
  if (options.target === "paper" && options.page) {
    parts.push(pageRule(options.page, options.pageNumbers, options.nativeMargins));
  }
  return parts.filter((part) => part !== "").join("\n");
}
