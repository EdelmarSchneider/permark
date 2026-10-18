// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { lineStaysUnbroken, markSolidLines } from "./codeLines";

describe("lineStaysUnbroken", () => {
  it("lets ordinary prose and ordinary code wrap", () => {
    expect(lineStaysUnbroken("A long paragraph that is one source line of ordinary prose.")).toBe(
      false,
    );
    expect(lineStaysUnbroken("def hi():")).toBe(false);
    expect(lineStaysUnbroken("    return foo->bar;")).toBe(false);
    expect(lineStaysUnbroken("return foo -> bar")).toBe(false);
    expect(lineStaysUnbroken("a || b")).toBe(false);
    expect(lineStaysUnbroken("")).toBe(false);
  });

  it("keeps a table row and a line of diagram source intact", () => {
    expect(lineStaysUnbroken("| name | purpose | notes |")).toBe(true);
    expect(lineStaysUnbroken("| --- | --- |")).toBe(true);
    expect(lineStaysUnbroken("Alice --> Bob: a long message that stays on one line")).toBe(true);
    expect(lineStaysUnbroken("Alice -> Bob: hello")).toBe(true);
    expect(lineStaysUnbroken("graph TD")).toBe(true);
    expect(lineStaysUnbroken("├── lib/")).toBe(true);
    expect(lineStaysUnbroken("+--------+")).toBe(true);
  });
});

function codeWith(html: string): HTMLElement {
  const code = document.createElement("code");
  code.innerHTML = html;
  return code;
}

describe("markSolidLines", () => {
  it("wraps only the lines that must not wrap, and keeps every character", () => {
    const prose = "A long paragraph that is one source line of ordinary prose.";
    const table = "| name | a very wide cell | notes |";
    const diagram = "Alice --> Bob: hello";
    const code = codeWith(`${prose}\n${table}\nmore prose here\n${diagram}\n`);
    const source = code.textContent;
    markSolidLines(code);
    expect(code.textContent).toBe(source);
    expect(
      Array.from(code.querySelectorAll(".code-line--unbroken"), (el) => el.textContent),
    ).toEqual([table, diagram]);
    expect(code.querySelector(".code-line--unbroken")?.textContent).not.toContain("paragraph");
  });

  it("keeps highlight markup around a line that must not wrap", () => {
    const code = codeWith('<span class="hljs-string">keep this\n| a | b |</span>');
    markSolidLines(code);
    const solid = code.querySelector(".code-line--unbroken");
    expect(solid?.textContent).toBe("| a | b |");
    expect(solid?.closest(".hljs-string")).not.toBeNull();
    expect(code.textContent).toBe("keep this\n| a | b |");
  });

  it("keeps the tokens of a row that highlight split into several spans", () => {
    const code = codeWith(
      'prose here\n<span class="hljs-punctuation">|</span> a <span class="hljs-string">| b |</span>\n',
    );
    markSolidLines(code);
    const solid = code.querySelector(".code-line--unbroken");
    expect(solid?.textContent).toBe("| a | b |");
    expect(solid?.querySelector(".hljs-punctuation")?.textContent).toBe("|");
    expect(solid?.querySelector(".hljs-string")?.textContent).toBe("| b |");
    expect(code.textContent).toBe("prose here\n| a | b |\n");
  });

  it("does not nest marks when the block is drawn again", () => {
    const code = codeWith("prose\n| a | b |\n");
    markSolidLines(code);
    markSolidLines(code);
    expect(code.querySelectorAll(".code-line--unbroken")).toHaveLength(1);
    expect(code.querySelector(".code-line--unbroken .code-line--unbroken")).toBeNull();
    expect(code.textContent).toBe("prose\n| a | b |\n");
  });
});
