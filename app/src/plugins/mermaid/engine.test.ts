// @vitest-environment jsdom
// Mermaid's palette: read from the page's theme and tone, or from a fixed theme when the caller forces one (a copy of
// the page in a look of its own must never take the colors, or a cached drawing, of the theme on screen).

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Init = {
  themeVariables: { darkMode: boolean; background: string } & Record<string, unknown>;
  gantt?: { useWidth?: number; fontSize?: number; sectionFontSize?: number };
  sankey?: { labelStyle?: string };
  themeCSS?: string;
};
const initialized: Init[] = [];
let rendered = 0;

vi.mock("mermaid", () => ({
  default: {
    initialize: (config: Init) => initialized.push(config),
    render: (id: string) => {
      rendered += 1;
      const background = initialized.at(-1)?.themeVariables.background ?? "";
      return Promise.resolve({
        svg: `<svg xmlns="http://www.w3.org/2000/svg" id="${id}" data-bg="${background}"></svg>`,
      });
    },
  },
}));

const { drawMermaid } = await import("./engine");
const { diagrams } = await import("../../core/doc/diagram");

const TOKENS = `
  :root, [data-theme="light"] {
    --bg: #ffffff; --text: #1b1d23; --measure: 760px;
    --tag-gray: #515962; --tag-red: #a81f26; --tag-orange: #9a4108; --tag-yellow: #7d5700;
    --tag-green: #146229; --tag-teal: #0a6560; --tag-blue: #1a58b8; --tag-purple: #6b3fc4;
  }
  :root[data-tone="paper"]:not([data-theme="dark"]) { --bg: #f6f0e1; }
  [data-theme="dark"] { --bg: #16171a; }
`;

beforeEach(() => {
  const style = document.createElement("style");
  style.textContent = TOKENS;
  document.head.append(style);
  initialized.length = 0;
  rendered = 0;
  diagrams.clear();
});
afterEach(() => {
  document.head.replaceChildren();
  document.body.replaceChildren();
  delete document.documentElement.dataset["theme"];
  delete document.documentElement.dataset["tone"];
});

describe("mermaid palette", () => {
  it("a forced light theme takes the plain light palette while the page is dark", async () => {
    document.documentElement.dataset["theme"] = "dark";
    const svg = await drawMermaid("graph TD; A-->B", "light", true);
    expect(svg).toContain('data-bg="#ffffff"');
    expect(initialized.at(-1)?.themeVariables.darkMode).toBe(false);
    // the probe the palette was read from is gone
    expect(document.body.children).toHaveLength(0);
  });

  it("a forced theme ignores the light tone and never shares a drawing with the page's", async () => {
    document.documentElement.dataset["theme"] = "light";
    document.documentElement.dataset["tone"] = "paper";
    const page = await drawMermaid("graph TD; A-->B", "light", false);
    const paper = await drawMermaid("graph TD; A-->B", "light", true);
    expect(page).toContain('data-bg="#f6f0e1"');
    expect(paper).toContain('data-bg="#ffffff"');
    expect(rendered).toBe(2);
    // asked again, each comes from the cache
    await drawMermaid("graph TD; A-->B", "light", true);
    await drawMermaid("graph TD; A-->B", "light", false);
    expect(rendered).toBe(2);
  });

  it("charts mark in the tag colors with the page color on them, and a gantt is laid out for the column", async () => {
    document.documentElement.dataset["theme"] = "light";
    await drawMermaid("pie\n a: 1", "light", false);
    const config = initialized.at(-1);
    const vars = config?.themeVariables ?? { darkMode: false, background: "" };
    const marks = ["#1a58b8", "#9a4108", "#146229", "#6b3fc4", "#0a6560", "#a81f26", "#7d5700"];
    // pie slices opaque (a see-through slice loses its contrast), their labels in the page color
    expect([1, 2, 3, 4, 5, 6, 7].map((n) => vars[`pie${n}`])).toEqual(marks);
    expect(vars["pieOpacity"]).toBe("1");
    expect(vars["pieSectionTextColor"]).toBe("#ffffff");
    expect([vars["pieTitleTextColor"], vars["pieLegendTextColor"]]).toEqual(["#1b1d23", "#1b1d23"]);
    // xychart's own palette is pastel: nearly invisible on a light page
    expect(vars["xyChart"]).toEqual({ plotColorPalette: [...marks, "#515962"].join(",") });
    // gantt bars: a tint of the tag color outlined in it; every task text in the page's text color (Mermaid writes
    // the text of an active | done | critical task in one color inside and beside its bar)
    expect(vars["taskBkgColor"]).toBe("#afc5e6");
    expect(vars["taskBorderColor"]).toBe("#1a58b8");
    expect(vars["activeTaskBkgColor"]).toBe("#a9c9c7");
    expect(vars["doneTaskBkgColor"]).toBe("#c2c5c8");
    expect(vars["critBkgColor"]).toBe("#e1b1b3");
    expect(vars["critBorderColor"]).toBe("#a81f26");
    for (const text of [
      "taskTextColor",
      "taskTextDarkColor",
      "taskTextLightColor",
      "taskTextOutsideColor",
    ])
      expect(vars[text], text).toBe("#1b1d23");
    // drawn at the reading column's width, not the window's (then scaled down to unreadable text)
    expect(config?.gantt?.useWidth).toBe(760);
    expect(config?.gantt?.fontSize).toBeGreaterThanOrEqual(13);
    // a sankey label reads over any flow: outlined in the page color, drawn once
    expect(config?.themeCSS ?? "").toMatch(
      /\.node-labels text \{[^}]*fill: #1b1d23;[^}]*stroke: #ffffff;[^}]*paint-order: stroke/,
    );
    expect(config?.sankey?.labelStyle).toBeUndefined();
    expect(config?.themeCSS ?? "").not.toContain("mix-blend-mode");
  });

  it("sankey flows are not multiplied into a dark page", async () => {
    document.documentElement.dataset["theme"] = "dark";
    await drawMermaid("sankey\na,b,1", "dark", false);
    expect(initialized.at(-1)?.themeCSS).toMatch(/\.link\s*\{[^}]*mix-blend-mode:\s*normal/);
  });
});
