import { describe, expect, it } from "vitest";
import { diagramAt, mermaidAt } from "./mermaidAt";

const doc = [
  "# T",
  "",
  "```mermaid",
  "graph LR",
  "  A --> B",
  "```",
  "",
  "```js",
  "mermaid",
  "```",
  "",
  "~~~mermaid",
  "mindmap",
  "  root",
  "~~~",
].join("\n");

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

describe("diagramAt", () => {
  it("names a PlantUML fence, including the short tag, and leaves the body", () => {
    const text = "```plantuml\nAlice -> Bob: hi\n@enduml\n```";
    expect(diagramAt(text, 2)).toEqual({
      source: "Alice -> Bob: hi\n@enduml",
      startLine: 1,
      kind: "plantuml",
    });
    expect(diagramAt("~~~puml\nA -> B\n~~~", 2)?.kind).toBe("plantuml");
    expect(diagramAt("```PUML\nA -> B\n```", 2)?.kind).toBe("plantuml");
  });

  it("keeps a Mermaid fence as mermaid and ignores a lookalike language", () => {
    expect(diagramAt(doc, 4)?.kind).toBe("mermaid");
    expect(diagramAt("```plantumlish\nA -> B\n```", 2)).toBeNull();
    expect(diagramAt("```js\nplantuml\n```", 2)).toBeNull();
  });
});
