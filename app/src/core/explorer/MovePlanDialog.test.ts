import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { declarations, ruleBody, styleOf } from "../testing/cssRules";

const style = styleOf(readFileSync(new URL("./MovePlanDialog.svelte", import.meta.url), "utf8"));

describe("the move dialog's height", () => {
  it("never grows past the window: the backdrop's one row is the window's height, the dialog fits in it", () => {
    // an auto row grows to the dialog's content, and a percentage height against it caps nothing
    expect(declarations(ruleBody(style, ".backdrop"))).toContain(
      "grid-template-rows: minmax(0, 1fr)",
    );
    expect(declarations(ruleBody(style, ".dialog"))).toContain("max-height: min(640px, 100%)");
  });

  it("scrolls a long list of links or skipped files inside, so the answers stay on screen", () => {
    for (const part of [".list", ".skipped"]) {
      const rules = declarations(ruleBody(style, part));
      expect(rules, part).toContain("min-height: 0");
      expect(rules, part).toContain("overflow: auto");
    }
    expect(declarations(ruleBody(style, "footer"))).toContain("flex-shrink: 0");
  });
});
