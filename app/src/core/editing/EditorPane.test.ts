import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { declarations, ruleBody, styleOf } from "../testing/cssRules";

const style = styleOf(
  readFileSync(join(process.cwd(), "src/core/editing/EditorPane.svelte"), "utf8"),
);

describe("the failed save's bar", () => {
  it("wraps a long unbroken reason (a path, a system message) inside the bar", () => {
    const rules = declarations(ruleBody(style, ".error span"));
    expect(rules).toContain("min-width: 0");
    expect(rules).toContain("overflow-wrap: anywhere");
  });
});
