// Graphviz (DOT), drawn in the page by the Graphviz build PlantUML already ships (Viz.js, WebAssembly): nothing is
// downloaded, no program runs, and the engine sees no file of the machine (its file system is an empty one in
// memory). The layout runs on the page's thread, so a graph too big to lay out in a moment is refused instead.

import {
  diagrams,
  paletteKey,
  sanitizeSvg,
  withTokens,
  type DrawFailure,
} from "../../core/doc/diagram";
import { t } from "../../core/i18n";
import type { Viz } from "@plantuml/core/viz-global.js";

/** Longest source drawn (~50 KB of DOT is a few thousand nodes, about a second of layout). */
const MAX_SOURCE = 50_000;
/** Most edges + statements drawn: `dot` lays out ~2 500 of them in a third of a second. */
const MAX_PARTS = 3_000;
/** The same for the slow layouts (force-directed `fdp`, `circo`) and the settings that multiply layout work. */
const MAX_PARTS_SLOW = 600;
const SLOW =
  /\b(?:layout\s*=\s*"?(?:fdp|circo)\b|mclimit|nslimit1?|searchsize|maxiter|remincross)/i;
/** A color the engine reads and that cannot end the quoted value it is written in. */
const COLOR = /^#[0-9a-f]{6}(?:[0-9a-f]{2})?$/i;
/** Graphviz measures text with built-in metrics; Helvetica's are the ones of a plain sans-serif. */
const FONT = "Helvetica";

let engine: Promise<Viz> | null = null;

function loadEngine(): Promise<Viz> {
  engine ??= import("@plantuml/core/viz-global.js")
    .then((module) => {
      // a bundler hands over the exports; loaded as a plain script, the file sets a global instead
      const make =
        typeof module.instance === "function" ? module.instance : globalThis.Viz?.instance;
      if (make === undefined) throw new Error("Graphviz did not load");
      return make();
    })
    .catch((error: unknown) => {
      engine = null;
      throw error;
    });
  return engine;
}

/** Whether a DOT source is too big to lay out on the page's thread without stalling the window. */
export function dotTooLarge(source: string): boolean {
  if (source.length > MAX_SOURCE) return true;
  const parts = source.match(/->|--|;|\n/g)?.length ?? 0;
  return parts > (SLOW.test(source) ? MAX_PARTS_SLOW : MAX_PARTS);
}

/** Where the body of the top graph starts (just after its `{`), past comments and quoted | HTML names; -1: none. */
function bodyStart(source: string): number {
  let i = 0;
  while (i < source.length) {
    const c = source[i];
    if (c === "{") return i + 1;
    if (c === '"') {
      i += 1;
      while (i < source.length && source[i] !== '"') i += source[i] === "\\" ? 2 : 1;
      i += 1;
    } else if (c === "<") {
      let depth = 0;
      do {
        if (source[i] === "<") depth += 1;
        else if (source[i] === ">") depth -= 1;
        i += 1;
      } while (i < source.length && depth > 0);
    } else if (source.startsWith("/*", i)) {
      const end = source.indexOf("*/", i + 2);
      i = end < 0 ? source.length : end + 2;
    } else if (c === "#" || source.startsWith("//", i)) {
      const end = source.indexOf("\n", i);
      i = end < 0 ? source.length : end;
    } else i += 1;
  }
  return -1;
}

/** ` name="value"` for each color read; a token that is not a plain hex color is left out (the engine's default). */
function colors(pairs: Record<string, string>): string {
  return Object.entries(pairs)
    .filter(([, value]) => COLOR.test(value))
    .map(([name, value]) => ` ${name}="${value}"`)
    .join("");
}

/**
 * The source with the page's palette as the graph's defaults: statements put first in the top graph, on the line of
 * its `{` (the engine's line numbers stay those of the source), so every color the source sets wins.
 */
function themed(source: string, token: (name: string) => string): string {
  const at = bodyStart(source);
  if (at < 0) return source;
  const text = token("--text");
  const line = token("--muted");
  const font = `fontname="${FONT}"`;
  const graph = colors({ color: token("--border-strong"), fontcolor: text });
  const node = colors({ color: line, fillcolor: token("--surface"), fontcolor: text });
  const edge = colors({ color: line, fontcolor: text });
  const defaults = `graph [bgcolor="transparent" ${font}${graph}] node [${font}${node}] edge [${font}${edge}]`;
  return `${source.slice(0, at)} ${defaults} ${source.slice(at)}`;
}

/**
 * The SVG for a DOT source in this theme; a failure with the engine's words when the graph is wrong | too big.
 * `forced`: the caller fixed the theme, not the page (its plain palette, whatever the page shows).
 */
export function drawDot(
  source: string,
  theme: "light" | "dark",
  forced = false,
): Promise<string | DrawFailure | null> {
  if (dotTooLarge(source)) return Promise.resolve({ problem: t("graphviz.tooLarge") });
  return diagrams.draw<string | DrawFailure | null>(
    `dot\u0000${paletteKey(theme, forced)}\u0000${source}`,
    async () => {
      const viz = await loadEngine();
      const result = viz.render(
        withTokens(theme, forced, (token) => themed(source, token)),
        {
          format: "svg",
        },
      );
      if (result.status === "failure") {
        const problem = result.errors.find((e) => e.level === "error")?.message;
        return problem === undefined ? null : { problem };
      }
      return sanitizeSvg(result.output);
    },
  );
}
