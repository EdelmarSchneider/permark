// PlantUML, drawn in the page by the official browser engine. Nothing is fetched: a directive that would load a URL is
// refused before the engine runs, and a standard-library include is refused the same way. The diagram kinds the
// engine draws on its own (sequence, class, activity, state, component) need neither.

import { diagrams, sanitizeSvg, type DrawFailure } from "../../core/doc/diagram";
import { t } from "../../core/i18n";

/** Longest source drawn: the engine and its layout run on the page's thread, which a long diagram would hold up. */
const MAX_SOURCE = 50_000;
/** Most lines + links drawn (a few thousand messages | classes already take seconds to lay out). */
const MAX_PARTS = 3_000;

/** Whether a PlantUML source is too big to draw on the page's thread without stalling the window. */
export function plantumlTooLarge(source: string): boolean {
  if (source.length > MAX_SOURCE) return true;
  const parts = source.match(/->|--|\.\.>|\n/g)?.length ?? 0;
  return parts > MAX_PARTS;
}

/** Whether the source asks the engine to load something from outside (an include, an import, a remote theme). */
export function plantumlFetches(source: string): boolean {
  return source
    .split(/\r?\n/)
    .some((line) =>
      /^\s*!(?:include(?:url|_url|_once|_many|sub)?|import)\b|^\s*!theme\b[^\n]*\bfrom\s+https?:\/\//i.test(
        line,
      ),
    );
}

type RenderToString = (
  lines: string[],
  onSuccess: (svg: string) => void,
  onError: (message: string) => void,
  options?: { dark?: boolean },
) => void;

let engine: Promise<RenderToString> | null = null;

/** Refuses every standard-library bundle, so the engine never inserts a script tag or opens the network. */
function installStdlibLoader(): void {
  // always set: an element with that id in a document is also a global name, and must not stand in for the loader
  Reflect.set(
    globalThis,
    "PLANTUML_STDLIB_LOADER",
    (_url: string, _ok: () => void, err: (message: string) => void) => {
      err("This diagram library is not available offline.");
    },
  );
}

function loadEngine(): Promise<RenderToString> {
  engine ??= Promise.all([import("@plantuml/core"), import("@plantuml/core/viz-global.js")]).then(
    ([module, viz]) => {
      if (typeof viz.instance === "function") Reflect.set(globalThis, "PLANTUML_VIZ", viz);
      installStdlibLoader();
      return module.renderToString;
    },
  );
  return engine;
}

/**
 * The engine's build line atop its error picture (`PlantUML version $version$ / $git.commit.id$ ...`, in italics): its
 * placeholders mean nothing to the reader. A diagram's own labels are never italic text of this shape.
 */
const BUILD_LINE = /<text\b[^>]*\bfont-style="italic"[^>]*>PlantUML version [^<]*<\/text>/;

/** The error picture shows the message and the lines around it only. */
function withoutBuildLine(svg: string): string {
  return svg.replace(BUILD_LINE, "");
}

/**
 * The SVG for a PlantUML source in this theme; null when it cannot be drawn or asks for something off the machine; a
 * failure with its words when it is too big to draw here.
 */
export function drawPlantuml(
  source: string,
  theme: "light" | "dark",
): Promise<string | DrawFailure | null> {
  if (plantumlFetches(source)) return Promise.resolve(null);
  if (plantumlTooLarge(source)) return Promise.resolve({ problem: t("plantuml.tooLarge") });
  return diagrams.draw<string | null>(`plantuml\u0000${theme}\u0000${source}`, () =>
    loadEngine().then(
      (renderToString) =>
        new Promise<string | null>((resolve) => {
          try {
            renderToString(
              source.split(/\r?\n/),
              (svg) => resolve(sanitizeSvg(withoutBuildLine(svg))),
              () => resolve(null),
              { dark: theme === "dark" },
            );
          } catch {
            resolve(null);
          }
        }),
    ),
  );
}
