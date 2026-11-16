// @vitest-environment jsdom
// Graphviz through the real engine: a drawing in the page's colors, the engine's own words when a graph is wrong, a
// graph too big to lay out on the page's thread refused at once, and nothing in a drawing that could run or navigate.

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { diagrams } from "../../core/doc/diagram";
import { dotTooLarge, drawDot } from "./engine";

const TOKENS = `
  :root, [data-theme="light"] { --bg: #ffffff; --surface: #f7f7f8; --border-strong: #d4d4d9; --text: #1f2328; --muted: #656d76; }
  [data-theme="dark"] { --bg: #16171a; --surface: #1c1d21; --border-strong: #3a3d44; --text: #e6e6e6; --muted: #9aa0a6; }
`;

beforeEach(() => {
  const style = document.createElement("style");
  style.textContent = TOKENS;
  document.head.append(style);
  diagrams.clear();
});
afterEach(() => {
  document.head.replaceChildren();
  document.body.replaceChildren();
  delete document.documentElement.dataset["theme"];
});

/** The drawing, parsed (fails the test when the engine gave no SVG). */
async function drawn(source: string, theme: "light" | "dark" = "light"): Promise<Element> {
  const svg = await drawDot(source, theme, true);
  if (typeof svg !== "string") throw new Error(`not drawn: ${JSON.stringify(svg)}`);
  return new DOMParser().parseFromString(svg, "image/svg+xml").documentElement;
}

describe("Graphviz drawings", () => {
  it("draws a small graph as SVG with its labels", async () => {
    const svg = await drawn('digraph { a -> b; b [label="Second"] }');
    expect(svg.localName).toBe("svg");
    const labels = [...svg.querySelectorAll("text")].map((text) => text.textContent);
    expect(labels).toEqual(expect.arrayContaining(["a", "Second"]));
  });

  it("an undirected graph and a quoted name holding a brace draw too", async () => {
    const svg = await drawn('strict graph "a{b" { x -- y }');
    expect(svg.querySelectorAll("g.node")).toHaveLength(2);
  });

  it("a graph the engine cannot read says why, never draws its source", async () => {
    const failure = await drawDot("digraph { a -> ", "light", true);
    expect(failure).toEqual({ problem: expect.stringMatching(/syntax error in line 1/) });
  });

  it("takes the colors of the theme it draws in; the graph's own colors win", async () => {
    const dark = await drawn("digraph { a -> b; c [color=red fontcolor=blue] }", "dark");
    const text = dark.querySelector("g.node text");
    expect(text?.getAttribute("fill")).toBe("#e6e6e6");
    expect(dark.querySelector("g.edge path")?.getAttribute("stroke")).toBe("#9aa0a6");
    const own = [...dark.querySelectorAll("g.node")].find(
      (node) => node.querySelector("title")?.textContent === "c",
    );
    expect(own?.querySelector("ellipse")?.getAttribute("stroke")).toBe("red");
    expect(own?.querySelector("text")?.getAttribute("fill")).toBe("blue");
    const light = await drawn("digraph { a -> b }", "light");
    expect(light.querySelector("g.node text")?.getAttribute("fill")).toBe("#1f2328");
  });

  it("the theme leaves the engine's line numbers as written", async () => {
    const failure = await drawDot("digraph {\n  a -> b\n  -> ;\n}", "dark", true);
    expect(failure).toEqual({ problem: expect.stringMatching(/line 3/) });
  });

  it("a crafted graph keeps no script, no handler and no script address; a web link goes to the page's handling", async () => {
    const svg = await drawn(
      `digraph {
        a [URL="javascript:alert(1)" label="A"];
        b [href=" java\tscript:alert(2)"];
        c [URL="https://example.com/x" target="_blank"];
        d [id="x\\" onmouseover=\\"alert(3)"];
        a -> b [URL="vbscript:msgbox"];
      }`,
    );
    const markup = new XMLSerializer().serializeToString(svg);
    expect(markup).not.toMatch(/script:/i);
    expect(svg.querySelector("script, foreignObject")).toBeNull();
    for (const element of svg.querySelectorAll("*")) {
      for (const attr of element.attributes) {
        expect(attr.name.toLowerCase().startsWith("on"), attr.name).toBe(false);
      }
      expect(element.hasAttribute("xlink:href"), element.localName).toBe(false);
    }
    const links = [...svg.querySelectorAll("a[href]")].map((a) => a.getAttribute("href"));
    expect(links).toEqual(["https://example.com/x"]);
  });

  it("a graph too big to lay out on the page's thread is refused before the engine runs", async () => {
    const edges = Array.from({ length: 4000 }, (_, i) => `n${i} -> n${i + 1};`).join("\n");
    const huge = `digraph {\n${edges}\n}`;
    expect(dotTooLarge(huge)).toBe(true);
    expect(dotTooLarge(`digraph { ${"a".repeat(60_000)} }`)).toBe(true);
    // a slow layout gets a smaller budget
    const medium = Array.from({ length: 400 }, (_, i) => `n${i} -> n${i + 1};`).join("\n");
    expect(dotTooLarge(`digraph {\n${medium}\n}`)).toBe(false);
    expect(dotTooLarge(`digraph { layout=fdp\n${medium}\n}`)).toBe(true);
    const started = performance.now();
    const failure = await drawDot(huge, "light", true);
    expect(failure).toEqual({ problem: "This graph is too large to draw here." });
    expect(performance.now() - started).toBeLessThan(100);
  });
});
