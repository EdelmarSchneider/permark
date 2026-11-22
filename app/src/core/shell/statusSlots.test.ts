// The status bar's slots in a narrow window: an item never wraps its words onto a second line of the 24 px bar (cut
// in half); where a slot has no room, whole items go, the last ones (highest order: least important) first.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/** The style rules of the window's template, one `selector -> body` entry each. */
function rules(): Map<string, string> {
  const app = readFileSync(resolve(process.cwd(), "src/App.svelte"), "utf8");
  const css = app
    .slice(app.indexOf("<style>") + "<style>".length, app.indexOf("</style>"))
    .replace(/\/\*[\s\S]*?\*\//g, "");
  const found = new Map<string, string>();
  for (const match of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const body = match[2] ?? "";
    for (const selector of (match[1] ?? "").split(",")) found.set(selector.trim(), body);
  }
  return found;
}

describe("the status bar's slots", () => {
  it("an item keeps its width: it never shrinks, so its words never wrap", () => {
    expect(rules().get(".slot > :global(*)")).toMatch(/flex-shrink:\s*0;/);
  });

  it("the left and center slots move what has no room to a second line the bar's height hides", () => {
    for (const slot of [".slot.left", ".slot.center"]) {
      const body = rules().get(slot) ?? "";
      expect(body, slot).toMatch(/flex-wrap:\s*wrap;/);
      expect(body, slot).toMatch(/height:\s*var\(--h-status\);/);
      expect(body, slot).toMatch(/overflow:\s*hidden;/);
    }
  });
});
