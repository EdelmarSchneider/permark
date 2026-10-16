import { ensureSyntaxTree } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { classHighlighter, highlightTree } from "@lezer/highlight";
import { describe, expect, it } from "vitest";
import { codeLanguage, sourceLanguage } from "./sourceHeadings";

describe("the language of the code inside a fence in Source mode", () => {
  const nameOf = (info: string): string | undefined => codeLanguage(info)?.name;

  it("is the language the fence names", () => {
    expect(nameOf("python")).toBe("Python");
    expect(nameOf("rust")).toBe("Rust");
    expect(nameOf("javascript")).toBe("JavaScript");
    expect(nameOf("css")).toBe("CSS");
  });

  it("reads the info string the way the reading view does", () => {
    expect(nameOf("py")).toBe("Python");
    expect(nameOf("PY")).toBe("Python");
    expect(nameOf("{python}")).toBe("Python");
    expect(nameOf("rust,ignore")).toBe("Rust");
    expect(nameOf("js:app.js")).toBe("JavaScript");
    expect(nameOf("ts")).toBe("TypeScript");
    expect(nameOf("golang")).toBe("Go");
  });

  it("is none for plain text, for a name nobody knows and for no name", () => {
    expect(codeLanguage("")).toBeNull();
    expect(codeLanguage("text")).toBeNull();
    expect(codeLanguage("txt")).toBeNull();
    expect(codeLanguage("zzz")).toBeNull();
  });

  /** The highlight classes of a text's stretches, as `text=class`. */
  function highlighted(doc: string): string[] {
    const state = EditorState.create({ doc, extensions: [sourceLanguage()] });
    const tree = ensureSyntaxTree(state, doc.length, 10_000);
    const found: string[] = [];
    if (tree) {
      highlightTree(tree, classHighlighter, (from, to, classes) =>
        found.push(`${doc.slice(from, to)}=${classes}`),
      );
    }
    return found;
  }

  it("highlights the code of a fence with it, once the language has loaded", async () => {
    await codeLanguage("python")?.load();
    const found = highlighted("# T\n\n```py\ndef f():\n    return 1\n```\n");
    expect(found).toContain("def=tok-keyword");
    expect(found).toContain("return=tok-keyword");
    expect(found).toContain("1=tok-number");
    // and the Markdown around it is still Markdown
    expect(found).toContain(" T=tok-heading");
  });

  it("leaves the code of a fence with no language, or a plain one, as text", async () => {
    await codeLanguage("python")?.load();
    for (const info of ["", "text"]) {
      const found = highlighted(`\`\`\`${info}\ndef f():\n    return 1\n\`\`\`\n`);
      expect(
        found.some((entry) => entry.startsWith("def=")),
        info,
      ).toBe(false);
    }
  });
});
