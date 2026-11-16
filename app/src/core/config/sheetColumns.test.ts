import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { declarations, ruleBody, rulesOf, styleOf } from "../testing/cssRules";

const tableSource = readFileSync(new URL("./SheetTable.svelte", import.meta.url), "utf8");
const table = styleOf(tableSource);
const value = styleOf(readFileSync(new URL("./SheetValue.svelte", import.meta.url), "utf8"));

const BREAKS_ANYWHERE = /overflow-wrap\s*:\s*anywhere|word-break\s*:\s*break-(?:all|word)/;

describe("sheet table columns", () => {
  it("keeps the words of a value whole, so the table cannot squeeze a column below its longest word", () => {
    const breaking = rulesOf(value).filter((rule) => BREAKS_ANYWHERE.test(rule.body));
    expect(breaking.map((rule) => rule.selectors.join(", "))).toEqual([]);
    expect(declarations(ruleBody(value, ".text"))).toContain("overflow-wrap: break-word");
  });

  it("gives every value cell a floor and scrolls a table wider than the pane sideways", () => {
    expect(tableSource).toMatch(/<td class="cell">\s*<SheetValue/);
    expect(declarations(ruleBody(table, ".cell"))).toContain("min-width: 10ch");
    expect(declarations(ruleBody(table, ".scroll-x"))).toContain("overflow-x: auto");
  });

  it("leaves the header and key cells as they were", () => {
    expect(declarations(ruleBody(table, "th"))).toContain("white-space: nowrap");
    expect(declarations(ruleBody(table, ".name"))).toContain("min-width: 8em");
  });
});
