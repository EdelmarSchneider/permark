import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const source = readFileSync(new URL("./VisualEditor.svelte", import.meta.url), "utf8");

/** The declarations of the rule for `selector` in the component's styles (comments removed). */
function declarations(selector: string): string[] {
  const css = (/<style>([\s\S]*)<\/style>/.exec(source)?.[1] ?? "").replace(
    /\/\*[\s\S]*?\*\//g,
    "",
  );
  const found: string[] = [];
  for (const match of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if ((match[1] ?? "").trim().replace(/\s+/g, " ") !== selector) continue;
    for (const part of (match[2] ?? "").split(";")) {
      const declaration = part.trim().replace(/\s+/g, " ");
      if (declaration !== "") found.push(declaration);
    }
  }
  return found;
}

describe("the editor's text", () => {
  it("keeps white space as typed: the browser puts no no-break space in for a second space", () => {
    // ProseMirror expects this (its own stylesheet sets it); without it a contenteditable writes U+00A0
    const rule = declarations(".host :global(.ProseMirror)");
    expect(rule).toContain("white-space: pre-wrap");
    expect(rule).toContain("word-wrap: break-word");
  });

  it("View's look drawn in a kept block | piece lays out its white space as View does", () => {
    // backend HTML has line breaks between its tags: kept as typed, they would show as empty lines
    expect(declarations(".host :global(:is(.pm-locked, .pm-inline))")).toContain(
      "white-space: normal",
    );
  });
});
