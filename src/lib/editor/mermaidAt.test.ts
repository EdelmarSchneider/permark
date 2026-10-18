import { describe, expect, it } from "vitest";
import { mermaidAt } from "./mermaidAt";

const doc = ["# T", "", "```mermaid", "graph LR", "  A --> B", "```", "", "```js", "mermaid", "```", "", "~~~mermaid", "mindmap", "  root", "~~~"].join(
  "\n",
);

describe("mermaidAt", () => {
  it("returns the diagram the caret is inside, including its fence lines", () => {
    expect(mermaidAt(doc, 4)).toEqual({ source: "graph LR\n  A --> B", startLine: 3 });
    expect(mermaidAt(doc, 3)?.startLine).toBe(3);
    expect(mermaidAt(doc, 6)?.source).toBe("graph LR\n  A --> B");
  });

  it("ignores a code block that is not a diagram, and a diagram the caret has left", () => {
    expect(mermaidAt(doc, 9)).toBeNull();
    expect(mermaidAt(doc, 1)).toBeNull();
  });

  it("finds a tilde fence after another diagram", () => {
    expect(mermaidAt(doc, 13)).toEqual({ source: "mindmap\n  root", startLine: 12 });
  });

  it("previews an unclosed fence up to the end of the text", () => {
    const open = "```mermaid\ngraph LR\n  A --> B";
    expect(mermaidAt(open, 2)).toEqual({ source: "graph LR\n  A --> B", startLine: 1 });
  });

  it("accepts an indented fence and a title after the language", () => {
    const text = "   ```mermaid title\ngraph TD\n```";
    expect(mermaidAt(text, 2)?.source).toBe("graph TD");
  });
});
