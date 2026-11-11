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

describe("the text column", () => {
  const markdownCss = readFileSync(new URL("../doc/markdown.css", import.meta.url), "utf8");

  it("is View's column: the reading width and zoom, with the side padding outside it", () => {
    // View: `.doc-column` pads the pane, `.md` inside it is the column (`--column` x `--zoom`) at most
    const column = /\.doc-column \{[^}]*padding: (\S+) (\S+) (\S+);/.exec(markdownCss);
    const side = column?.[2];
    expect(side).toBeDefined();
    const page = declarations(".page");
    expect(page).toContain(`padding: var(--sp-5) ${side} var(--sp-8)`);
    expect(page).toContain(`max-width: calc(var(--column) * var(--zoom) + 2 * ${side})`);
    // the reading width and zoom View's column takes
    expect(source).toMatch(/class="page md"\s+class:is-wide=\{reading\.width === "wide"\}/);
    expect(source).toMatch(/class:is-full=\{reading\.width === "full"\}/);
    expect(source).toMatch(/style:--zoom=\{reading\.zoom \/ 100\}/);
  });
});

describe("what the editor answers the session", () => {
  it("an edit counts as changeable while the editor cannot tell yet (not ready)", () => {
    // as the session answers with no editor on screen: `apply` still checks each edit; "no" here would turn every
    // part of a run into one "that can only be changed in Source"
    const answer = /changeable: \(range, insert\) => \{([\s\S]*?)\n {8}\},/.exec(source)?.[1] ?? "";
    expect(answer).toContain("mod.changeable(core, now, range, insert)");
    expect(answer.replace(/\s+/g, " ")).toMatch(/\? mod\.changeable\([^)]*\) : true;/);
  });
});

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
