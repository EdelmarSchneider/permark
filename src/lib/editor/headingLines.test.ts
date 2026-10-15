import { describe, expect, it } from "vitest";
import { headingLinesOf } from "./headingLines";

describe("headingLinesOf", () => {
  it("gives each heading its level, text and line, and the last line of the text", () => {
    const text = "# Plan\n\nIntro.\n\n## Rollout\n\nShip it.\n\n### Week 1\n\nStart.\n";
    expect(headingLinesOf(text)).toEqual({
      headings: [
        { level: 1, text: "Plan", line: 1 },
        { level: 2, text: "Rollout", line: 5 },
        { level: 3, text: "Week 1", line: 9 },
      ],
      lastLine: 12,
    });
  });

  it("skips front matter, code blocks and quotes, as the Outline does", () => {
    const text = [
      "---",
      "title: x",
      "# not a heading",
      "---",
      "",
      "# Real",
      "",
      "```",
      "# in a code block",
      "```",
      "",
      "> # in a quote",
      "",
      "Setext",
      "------",
      "",
    ].join("\n");
    const { headings } = headingLinesOf(text);
    expect(headings.map((h) => [h.text, h.line])).toEqual([
      ["Real", 6],
      ["Setext", 14],
    ]);
  });

  it("reads a text with no headings and an empty one", () => {
    expect(headingLinesOf("just a paragraph")).toEqual({ headings: [], lastLine: 1 });
    expect(headingLinesOf("")).toEqual({ headings: [], lastLine: 1 });
  });
});
