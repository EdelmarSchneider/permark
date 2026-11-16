import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("the reading menu", () => {
  const css = readFileSync(resolve(process.cwd(), "src/core/doc/ReadingMenu.svelte"), "utf8");
  const rule = (selector: string): string => {
    const at = css.split("\n").findIndex((line) => line === `  ${selector} {`);
    return at < 0 ? "" : (css.split("\n").slice(at).join("\n").split("}")[0] ?? "");
  };

  it("grows with its longest words (a long Reset zoom, three wide segments), never narrower than before, never past the window", () => {
    const menu = rule(".reading-menu");
    expect(menu).toMatch(/^\s*width: max-content;/m);
    expect(menu).toMatch(/^\s*min-width: 260px;/m);
    expect(menu).toMatch(/^\s*max-width: min\(\d+px, calc\(100vw - .+\)\);/m);
    expect(menu).not.toMatch(/^\s*width: 260px;/m);
  });
});
