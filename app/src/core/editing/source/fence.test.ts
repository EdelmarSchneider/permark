import { describe, expect, it } from "vitest";
import { fenceAround } from "./fence";

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
    });
    expect(fenceAround(DOC, 5)?.language).toBe("mermaid");
  });

  it("finds a plantuml block written with tildes", () => {
    expect(fenceAround(DOC, 9)).toEqual({
      language: "plantuml",
      code: "@startuml\n@enduml",
      from: 8,
    });
  });

  it("is nothing on the fence lines, outside blocks and in code of another language", () => {
    for (const line of [1, 2, 3, 6, 7, 8, 11, 13])
      expect(fenceAround(DOC, line), `${line}`).toBeNull();
  });

  it("a shorter fence inside a longer one does not close it", () => {
    expect(fenceAround(DOC, 17)).toEqual({ language: "mermaid", code: "```\ngraph LR", from: 15 });
  });

  it("an unclosed block runs to the end of what is given", () => {
    expect(fenceAround(["```puml", "@startuml", "A -> B"], 3)).toEqual({
      language: "puml",
      code: "@startuml\nA -> B",
      from: 1,
    });
  });

  it("odd input never breaks it", () => {
    expect(fenceAround([], 1)).toBeNull();
    expect(fenceAround(["```mermaid"], 0)).toBeNull();
    expect(fenceAround(["```MerMaid", "x", "```"], 2)?.language).toBe("mermaid");
    expect(fenceAround(["    ```mermaid", "x", "```"], 2)).toBeNull();
  });
});
