import { describe, expect, it } from "vitest";
import { dotTooLarge } from "../../../plugins/graphviz/engine";
import { fenceAround, fenceIn } from "./fence";

const DOC = [
  "# Title", // 1
  "", // 2
  "```mermaid", // 3
  "graph TD", // 4
  "  A --> B", // 5
  "```", // 6
  "", // 7
  "~~~ plantuml", // 8
  "@startuml", // 9
  "@enduml", // 10
  "~~~", // 11
  "```js", // 12
  "let x = 1;", // 13
  "```", // 14
  "````mermaid", // 15
  "```", // 16 (shorter: not a close)
  "graph LR", // 17
  "````", // 18
];

describe("the diagram fence the caret is in", () => {
  it("finds a mermaid block and its code", () => {
    expect(fenceAround(DOC, 4)).toEqual({
      language: "mermaid",
      code: "graph TD\n  A --> B",
      from: 3,
      closed: true,
    });
    expect(fenceAround(DOC, 5)?.language).toBe("mermaid");
  });

  it("finds a plantuml block written with tildes", () => {
    expect(fenceAround(DOC, 9)).toEqual({
      language: "plantuml",
      code: "@startuml\n@enduml",
      from: 8,
      closed: true,
    });
  });

  it("finds a Graphviz block by either of its names", () => {
    for (const language of ["dot", "graphviz"]) {
      expect(fenceAround([`\`\`\`${language}`, "digraph { a -> b }", "```"], 2)).toEqual({
        language,
        code: "digraph { a -> b }",
        from: 1,
        closed: true,
      });
    }
  });

  it("is nothing on the fence lines, outside blocks and in code of another language", () => {
    for (const line of [1, 2, 3, 6, 7, 8, 11, 13])
      expect(fenceAround(DOC, line), `${line}`).toBeNull();
  });

  it("a shorter fence inside a longer one does not close it", () => {
    expect(fenceAround(DOC, 17)).toEqual({
      language: "mermaid",
      code: "```\ngraph LR",
      from: 15,
      closed: true,
    });
  });

  it("an unclosed block runs to the end of what is given", () => {
    expect(fenceAround(["```puml", "@startuml", "A -> B"], 3)).toEqual({
      language: "puml",
      code: "@startuml\nA -> B",
      from: 1,
      closed: false,
    });
  });

  it("odd input never breaks it", () => {
    expect(fenceAround([], 1)).toBeNull();
    expect(fenceAround(["```mermaid"], 0)).toBeNull();
    expect(fenceAround(["```MerMaid", "x", "```"], 2)?.language).toBe("mermaid");
    expect(fenceAround(["    ```mermaid", "x", "```"], 2)).toBeNull();
  });
});

/** `lines` read as a document, 1-based, as the Source editor gives them. */
function fenceOf(lines: readonly string[], at: number) {
  return fenceIn((n) => lines[n - 1] ?? "", lines.length, at);
}

/** A page with a Graphviz block of `edges` lines (line 3 opens it; each line ~20 characters), text around it. */
function bigGraph(edges: number): string[] {
  const body = Array.from({ length: edges }, (_, i) => `  node${i} -> node${i + 1};`);
  return ["# Graph", "", "```dot", "digraph {", ...body, "}", "```", "", "after"];
}

describe("the diagram fence around the caret in a long document", () => {
  it("a block longer than the lines read around the caret is read whole (or past what any engine draws)", () => {
    const doc = bigGraph(3000);
    for (const at of [10, 1500]) {
      const fence = fenceOf(doc, at);
      expect(fence?.language, `${at}`).toBe("dot");
      expect(fence?.code.length, `${at}`).toBeGreaterThan(50_000);
      expect(fence?.code.startsWith("digraph {"), `${at}`).toBe(true);
      expect(dotTooLarge(fence?.code ?? ""), `${at}`).toBe(true);
    }
    // read to its closing fence: the whole block, nothing after it
    const whole = fenceOf(doc, 1500);
    expect(whole?.closed).toBe(true);
    expect(whole?.code).toBe(doc.slice(3, -3).join("\n"));
    expect(whole?.from).toBe(3);
  });

  it("an unclosed block running through a long document is read until past what any engine draws", () => {
    const doc = ["```dot", ...Array.from({ length: 20_000 }, (_, i) => `n${i} -> n${i + 1};`)];
    const fence = fenceOf(doc, 5);
    expect(fence?.closed).toBe(false);
    expect(fence?.code.length).toBeGreaterThan(50_000);
    expect(fence?.code.length).toBeLessThan(80_000);
    expect(dotTooLarge(fence?.code ?? "")).toBe(true);
  });

  it("a small block in a long document reads as before", () => {
    const filler = Array.from({ length: 2000 }, (_, i) => `text ${i}`);
    const doc = [...filler, "```mermaid", "graph TD", "  A --> B", "```", ...filler];
    expect(fenceOf(doc, 2002)).toEqual({
      language: "mermaid",
      code: "graph TD\n  A --> B",
      from: 2001,
      closed: true,
    });
    expect(fenceOf(doc, 2500)).toBeNull();
    expect(fenceOf(doc, 1500)).toBeNull();
  });

  it("the caret below a long block of another language | after a closed block is in no diagram", () => {
    const code = Array.from({ length: 3000 }, (_, i) => `let x${i} = ${i};`);
    expect(fenceOf(["```js", ...code, "```"], 1500)).toBeNull();
    const after = Array.from({ length: 3000 }, (_, i) => `text ${i}`);
    expect(fenceOf(["```dot", "digraph { a }", "```", ...after], 2000)).toBeNull();
  });
});
