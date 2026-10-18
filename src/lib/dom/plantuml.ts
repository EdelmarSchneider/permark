// PlantUML, drawn in the page by the official browser engine. Nothing is fetched: a directive that would
// load a URL is refused before the engine runs, and a standard-library include is refused the same way.
// The diagram kinds the engine draws on its own (sequence, class, activity, state, component) need neither.

export function plantumlFetches(source: string): boolean {
  return source
    .split(/\r?\n/)
    .some((line) =>
      /^\s*!(?:include(?:url|_url|_once|_many|sub)?|import)\b|^\s*!theme\b[^\n]*\bfrom\s+https?:\/\//i.test(
        line,
      ),
    );
}

const REMOTE_URL = /^(?:javascript:|data:)/i;
const REMOTE_ASSET = /^(?:javascript:|data:|https?:|\/\/)/i;

/** The engine sometimes writes `xmlns` twice. XML allows the attribute once, so the extras are dropped before parsing. */
function uniqueXmlns(svg: string): string {
  let seen = false;
  return svg.replace(/\sxmlns="[^"]*"/g, (match) => {
    if (seen) return "";
    seen = true;
    return match;
  });
}

/** The SVG the engine returned, with scripts and remote images removed, or null when it is not an SVG. */
export function sanitizePlantumlSvg(svg: string): string | null {
  const doc = new DOMParser().parseFromString(uniqueXmlns(svg), "image/svg+xml");
  const root = doc.documentElement;
  if (root.localName !== "svg" || doc.querySelector("parsererror")) return null;
  root
    .querySelectorAll("script, foreignObject, iframe, object, embed")
    .forEach((el) => el.remove());
  for (const el of root.querySelectorAll("*")) {
    for (const attr of [...el.attributes]) {
      if (attr.name.toLowerCase().startsWith("on")) el.removeAttribute(attr.name);
    }
    const href = el.getAttribute("href") ?? el.getAttribute("xlink:href");
    if (href === null) continue;
    const value = href.trim();
    const link = el.localName === "a";
    if (link ? REMOTE_URL.test(value) : REMOTE_ASSET.test(value)) {
      if (link) {
        el.removeAttribute("href");
        el.removeAttribute("xlink:href");
      } else {
        el.remove();
      }
    }
  }
  return new XMLSerializer().serializeToString(root);
}

type RenderToString = (
  lines: string[],
  onSuccess: (svg: string) => void,
  onError: (message: string) => void,
  options?: { dark?: boolean },
) => void;

type StdlibHost = typeof globalThis & {
  PLANTUML_STDLIB_LOADER?: (url: string, ok: () => void, err: (message: string) => void) => void;
  PLANTUML_VIZ?: { instance: () => Promise<unknown> };
};

let engine: Promise<RenderToString> | null = null;

/** Refuses every standard-library bundle, so the engine never inserts a script tag or opens the network. */
function installStdlibLoader(): void {
  const host = globalThis as StdlibHost;
  if (host.PLANTUML_STDLIB_LOADER) return;
  host.PLANTUML_STDLIB_LOADER = (_url, _ok, err) => {
    err("This diagram library is not available offline.");
  };
}

function loadEngine(): Promise<RenderToString> {
  engine ??= Promise.all([
    import("@plantuml/core"),
    import("@plantuml/core/viz-global.js"),
  ]).then(([mod, viz]) => {
    const host = globalThis as StdlibHost;
    if (typeof viz.instance === "function") host.PLANTUML_VIZ = viz;
    installStdlibLoader();
    return mod.renderToString;
  });
  return engine;
}

/** The drawing for `source`, or null when it cannot be drawn or it asks for something off the machine. */
export function renderPlantuml(source: string, theme: "light" | "dark"): Promise<string | null> {
  if (plantumlFetches(source)) return Promise.resolve(null);
  return loadEngine().then(
    (renderToString) =>
      new Promise((resolve) => {
        try {
          renderToString(
            source.split(/\r?\n/),
            (svg) => resolve(sanitizePlantumlSvg(svg)),
            () => resolve(null),
            { dark: theme === "dark" },
          );
        } catch {
          resolve(null);
        }
      }),
  );
}
