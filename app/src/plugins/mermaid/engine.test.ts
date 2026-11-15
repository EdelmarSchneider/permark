// @vitest-environment jsdom
// Mermaid's palette: read from the page's theme and tone, or from a fixed theme when the caller forces one (a copy of
// the page in a look of its own must never take the colors, or a cached drawing, of the theme on screen).

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Init = { themeVariables: { darkMode: boolean; background: string } };
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
  :root, [data-theme="light"] { --bg: #ffffff; }
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
});
