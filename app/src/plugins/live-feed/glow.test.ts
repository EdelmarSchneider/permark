import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("./glow.css", import.meta.url), "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** The declarations of each rule for `selector`, in sheet order (later ones win). */
function rulesFor(selector: string): string[][] {
  const found: string[][] = [];
  for (const match of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((s) => s.trim().replace(/\s+/g, " "));
    if (!selectors.includes(selector)) continue;
    found.push(
      (match[2] ?? "")
        .split(";")
        .map((part) => part.trim().replace(/\s+/g, " "))
        .filter((part) => part.length > 0),
    );
  }
  return found;
}

describe("the glow of a changed passage", () => {
  it("spills a little around a block, but stays inside a table's frame, which scrolls sideways", () => {
    expect(rulesFor(".md-body > .pm-glow::after").flat()).toContain("inset: -2px -8px");
    // anything outside the frame would be scrolled to: scroll bars would show while it fades
    const table = rulesFor(".md-body > .table-wrap.pm-glow::after").flat();
    expect(table).toContain("inset: 0");
    expect(css.indexOf(".md-body > .table-wrap.pm-glow::after")).toBeGreaterThan(
      css.indexOf(".md-body > .pm-glow::after"),
    );
  });
});
