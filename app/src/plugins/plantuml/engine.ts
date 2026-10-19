// PlantUML, drawn in the page by the official browser engine. Nothing is fetched: a directive that would load a URL is
// refused before the engine runs, and a standard-library include is refused the same way. The diagram kinds the
// engine draws on its own (sequence, class, activity, state, component) need neither.

import { diagrams, sanitizeSvg } from "../../core/doc/diagram";

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
  if (Reflect.has(globalThis, "PLANTUML_STDLIB_LOADER")) return;
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

/** The SVG for a PlantUML source in this theme, or null when it cannot be drawn or asks for something off the machine. */
export function drawPlantuml(source: string, theme: "light" | "dark"): Promise<string | null> {
  if (plantumlFetches(source)) return Promise.resolve(null);
  return diagrams.draw(`plantuml\u0000${theme}\u0000${source}`, () =>
    loadEngine().then(
      (renderToString) =>
        new Promise<string | null>((resolve) => {
          try {
            renderToString(
              source.split(/\r?\n/),
              (svg) => resolve(sanitizeSvg(svg)),
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
