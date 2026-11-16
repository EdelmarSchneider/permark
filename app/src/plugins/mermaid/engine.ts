// Mermaid, loaded when the first diagram comes near the screen. Strict security level; the palette comes from the
// design tokens so the diagram looks like the page in every theme and tone.

import { diagrams, paletteKey, sanitizeSvg, withTokens } from "../../core/doc/diagram";

type Mermaid = typeof import("mermaid").default;

let engine: Promise<Mermaid> | null = null;
let configuredFor: string | null = null;
let counter = 0;

/**
 * Chart marks (pie slices, xychart series, gantt task outlines): the tag colors, which keep 3:1 on the page and the
 * page color 4.5:1 on them in every theme. Mermaid derives its own from `primaryColor`, a surface: pale shades of one
 * gray.
 */
const MARKS = ["blue", "orange", "green", "purple", "teal", "red", "yellow", "gray"] as const;

/** Mermaid's gantt text size (its default, 11, is small even at full size). */
const GANTT_FONT = 14;

/** Share of a gantt bar's color over the page: the page's text keeps 6.5:1 on it in every theme and tone. */
const BAR_TINT = 0.35;

/** `color` laid over `page` at `BAR_TINT` (both `#rrggbb`; any other form: `color` itself). */
function tint(color: string, page: string): string {
  const channels = (hex: string): number[] | null =>
    /^#[0-9a-f]{6}$/i.test(hex)
      ? [1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16))
      : null;
  const [over, under] = [channels(color), channels(page)];
  if (over === null || under === null) return color;
  const mixed = over.map((c, i) => Math.round(c * BAR_TINT + (under[i] ?? 0) * (1 - BAR_TINT)));
  return `#${mixed.map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

async function load(theme: "light" | "dark", forced: boolean): Promise<Mermaid> {
  engine ??= import("mermaid").then((module) => module.default);
  const mermaid = await engine;
  const key = paletteKey(theme, forced);
  if (configuredFor !== key) {
    withTokens(theme, forced, (token) => {
      const mark = (name: (typeof MARKS)[number]): string => token(`--tag-${name}`);
      const marks = MARKS.map(mark);
      const onMark = token("--bg");
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        // labels are SVG text, not HTML in a foreignObject: the drawing is cleaned of foreign content before it enters the page
        htmlLabels: false,
        flowchart: { htmlLabels: false },
        // a gantt takes the width of the element it is drawn in (an off-screen one as wide as the window) and is then
        // scaled down to the column, its text with it: drawn at the column's width instead
        gantt: {
          useWidth: Number.parseFloat(token("--measure")) || 760,
          fontSize: GANTT_FONT,
          sectionFontSize: GANTT_FONT,
          barHeight: 24,
        },
        // sankey: a label on its flow gets an outline of the page color, so it reads over any flow (Mermaid's own
        // outlined labels draw each label twice); flows are multiplied into what is under them: on a dark page, black
        themeCSS: [
          `.node-labels text { fill: ${token("--text")}; stroke: ${token("--bg")}; stroke-width: 3px; stroke-linejoin: round; paint-order: stroke; }`,
          theme === "dark" ? ".link { mix-blend-mode: normal !important; }" : "",
        ].join(" "),
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
          ...Object.fromEntries(marks.map((color, i) => [`pie${i + 1}`, color])),
          // a see-through slice loses its contrast
          pieOpacity: "1",
          pieSectionTextColor: onMark,
          pieTitleTextColor: token("--text"),
          pieLegendTextColor: token("--text"),
          pieStrokeColor: token("--bg"),
          pieOuterStrokeColor: token("--border-strong"),
          xyChart: { plotColorPalette: marks.join(",") },
          // gantt: Mermaid writes the text of an active | done | critical task in one color inside its bar and beside
          // it, so a bar is a tint of its color (outlined in it) and every task text is the page's text color
          taskBkgColor: tint(mark("blue"), token("--bg")),
          taskBorderColor: mark("blue"),
          activeTaskBkgColor: tint(mark("teal"), token("--bg")),
          activeTaskBorderColor: mark("teal"),
          doneTaskBkgColor: tint(mark("gray"), token("--bg")),
          doneTaskBorderColor: mark("gray"),
          critBkgColor: tint(mark("red"), token("--bg")),
          critBorderColor: mark("red"),
          taskTextColor: token("--text"),
          taskTextDarkColor: token("--text"),
          taskTextLightColor: token("--text"),
          taskTextOutsideColor: token("--text"),
          sectionBkgColor: token("--surface-2"),
          sectionBkgColor2: token("--surface-2"),
          altSectionBkgColor: token("--bg"),
          excludeBkgColor: token("--surface"),
          gridColor: token("--border"),
          todayLineColor: mark("red"),
          vertLineColor: token("--muted"),
          titleColor: token("--text"),
        },
      });
    });
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
