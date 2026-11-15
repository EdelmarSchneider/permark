// Mermaid, loaded when the first diagram comes near the screen. Strict security level; the palette comes from the
// design tokens so the diagram looks like the page in every theme and tone.

import { diagrams, sanitizeSvg } from "../../core/doc/diagram";

type Mermaid = typeof import("mermaid").default;

let engine: Promise<Mermaid> | null = null;
let configuredFor: string | null = null;
let counter = 0;

/** A forced theme ignores the page's light tone: its drawings never share a cache entry with the page's. */
function paletteKey(theme: "light" | "dark", forced: boolean): string {
  return forced ? `${theme}:forced` : `${theme}:${document.documentElement.dataset["tone"] ?? ""}`;
}

/**
 * `read` gets the design tokens: the page's (theme + tone), or for a forced theme those of a probe element scoped to
 * that theme alone (the tokens scope each theme by `data-theme`; the tones apply to the root only).
 */
function withTokens<T>(
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

async function load(theme: "light" | "dark", forced: boolean): Promise<Mermaid> {
  engine ??= import("mermaid").then((module) => module.default);
  const mermaid = await engine;
  const key = paletteKey(theme, forced);
  if (configuredFor !== key) {
    withTokens(theme, forced, (token) =>
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        // labels are SVG text, not HTML in a foreignObject: the drawing is cleaned of foreign content before it enters the page
        htmlLabels: false,
        flowchart: { htmlLabels: false },
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
          // edge labels sit on the page, not on a derived shade
          edgeLabelBackground: token("--bg"),
          textColor: token("--text"),
          noteBkgColor: token("--surface-2"),
          noteTextColor: token("--text"),
        },
      }),
    );
    configuredFor = key;
  }
  return mermaid;
}

/**
 * The SVG for a Mermaid source in this theme, or null when Mermaid cannot draw it. `forced`: the caller fixed the
 * theme, not the page (its plain palette, whatever the page shows).
 */
export function drawMermaid(
  source: string,
  theme: "light" | "dark",
  forced = false,
): Promise<string | null> {
  return diagrams.draw(`mermaid\u0000${paletteKey(theme, forced)}\u0000${source}`, async () => {
    counter += 1;
    const id = `pm-mermaid-${counter}`;
    try {
      const mermaid = await load(theme, forced);
      const { svg } = await mermaid.render(id, source);
      return sanitizeSvg(svg);
    } catch {
      // Mermaid leaves its error graphic behind in the page when a render fails
      document.getElementById(id)?.remove();
      return null;
    } finally {
      document.getElementById(`d${id}`)?.remove();
    }
  });
}
