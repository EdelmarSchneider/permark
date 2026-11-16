import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("./markdown.css", import.meta.url), "utf8");
const math = readFileSync(new URL("../../plugins/math/Math.svelte", import.meta.url), "utf8");
const paper = readFileSync(new URL("../../plugins/export/paper.css", import.meta.url), "utf8");

/** CSS with comments removed, so a note cannot hide or satisfy a declaration. */
function codeOnly(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "");
}

type Rule = { selectors: string[]; body: string };

function rulesOf(source: string): Rule[] {
  const found: Rule[] = [];
  for (const match of codeOnly(source).matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "")
      .split(",")
      .map((selector) => selector.trim().replace(/\s+/g, " "))
      .filter((selector) => selector.length > 0);
    if (selectors.length === 0 || selectors.every((selector) => selector.startsWith("@"))) continue;
    found.push({ selectors, body: match[2] ?? "" });
  }
  return found;
}

function ruleBody(source: string, selector: string): string {
  return rulesOf(source)
    .filter((rule) => rule.selectors.includes(selector))
    .map((rule) => rule.body)
    .join("\n");
}

function declarations(body: string): string[] {
  return body
    .split(";")
    .map((part) => part.trim().replace(/\s+/g, " "))
    .filter((part) => part.length > 0);
}

const sheet = rulesOf(css);

/** `max-content` / `100cqw` / centering: the block grows out to the pane. */
const PANE_GROWTH =
  /(?:^|[\s;])(?:width|max-width)\s*:\s*(?:max-content|100cqw)\b|translateX\(\s*-50%\s*\)/;

describe("reading column", () => {
  it("keeps a top-level table, diagram, and lone image from growing out to the pane", () => {
    const breakout = [".md-body > .table-wrap", ".md-body > .diagram", ".md-body > .pm-wide"];
    const growing = breakout.filter((selector) => PANE_GROWTH.test(ruleBody(css, selector)));
    expect(growing).toEqual([]);
    expect(codeOnly(css)).not.toMatch(/100cqw/);
    expect(codeOnly(css)).not.toMatch(/width:\s*max-content/);
    expect(ruleBody(css, ".md")).toMatch(
      /max-width:\s*calc\(var\(--column\)\s*\*\s*var\(--zoom\)\)/,
    );
    const image = declarations(ruleBody(css, ".md img"));
    expect(image).toContain("max-width: 100%");
    expect(image.some((decl) => /^(?:min-)?width:/.test(decl))).toBe(false);
  });

  it("keeps words whole in screen cells (a wide table scrolls in its wrapper); paper wraps long tokens", () => {
    // `anywhere` | `break-all` let the table squeeze a column below its longest word: words break mid-word
    const breaking = sheet.filter(
      (rule) =>
        /overflow-wrap\s*:\s*anywhere|word-break\s*:\s*break-(?:all|word)/.test(rule.body) &&
        rule.selectors.some((selector) => /(?:^|\s)(?:th|td)$/.test(selector)),
    );
    expect(breaking.map((rule) => rule.selectors.join(", "))).toEqual([]);
    const shared = sheet.find(
      (rule) => rule.selectors.includes(".md th") && rule.selectors.includes(".md td"),
    );
    expect(shared).toBeDefined();
    expect(declarations(shared?.body ?? "")).toContain("min-width: 5em");
    // paper cannot scroll: its cells wrap
    expect(declarations(ruleBody(paper, ".md td"))).toContain("overflow-wrap: anywhere");
    expect(declarations(ruleBody(paper, ".md .table-wrap"))).toContain("overflow: visible");
  });

  it("keeps inner table scroll, the 5em floor, Full, code, and display math", () => {
    expect(declarations(ruleBody(css, ".md .table-wrap"))).toContain("overflow-x: auto");
    expect(declarations(ruleBody(css, ".md th"))).toContain("min-width: 5em");
    expect(declarations(ruleBody(css, ".md td"))).toContain("min-width: 5em");
    expect(declarations(ruleBody(css, ".md.is-full"))).toContain("--column: 100000px");
    expect(declarations(ruleBody(css, ".md.is-wide"))).toEqual(["--column: var(--measure-wide)"]);
    expect(declarations(ruleBody(css, ".md pre"))).toEqual([
      "margin: 0",
      "padding: 14px 16px",
      "max-width: 100%",
      "overflow-x: auto",
      "white-space: pre-wrap",
      "border-radius: var(--radius-card)",
      "background: var(--surface)",
      "border: 1px solid var(--border)",
      "font: calc(var(--fs-code) * var(--zoom)) / 1.6 var(--font-code)",
    ]);
    const katex = /:global\(\.md \.katex-display\)\s*\{([^}]*)\}/.exec(math)?.[1] ?? "";
    expect(declarations(katex)).toEqual([
      "margin: 0.8em 0",
      "overflow-x: auto",
      "overflow-y: hidden",
    ]);
  });

  it("aligns a table column as written (`:-:`, `--:`): the cells' align wins over the default", () => {
    // a cell's `align` attribute is a presentational hint, below any author rule: the shared rule sets the start
    // (a header cell is centered by the browser otherwise) and the written alignment is a rule of its own
    const shared = sheet.find(
      (rule) => rule.selectors.includes(".md th") && rule.selectors.includes(".md td"),
    );
    expect(declarations(shared?.body ?? "")).toContain("text-align: start");
    for (const side of ["left", "center", "right"]) {
      for (const cell of ["th", "td"]) {
        const selector = `.md ${cell}[align="${side}" i]`;
        expect(declarations(ruleBody(css, selector)), selector).toContain(`text-align: ${side}`);
      }
    }
  });
});
