import { describe, expect, it } from "vitest";
import { shownSummary, type TreeNode } from "./tree";

function node(kind: TreeNode["kind"], summary: string): TreeNode {
  return { label: null, kind, summary, children: null };
}

const decoded = "a\n\t\r\"\\\u0001";
const spelling = JSON.stringify(decoded);

describe("shownSummary", () => {
  it("returns the spelling when plain is off", () => {
    expect(shownSummary(node("string", spelling), false)).toBe(spelling);
    expect(shownSummary(node("string", '"\\u00e9"'), false)).toBe('"\\u00e9"');
    expect(spelling).toContain("\\n");
    expect(spelling).toContain('\\"');
    expect(spelling).toContain("\\\\");
    expect(spelling).toContain("\\u0001");
  });

  it("decodes a string when plain is on (newline, quotes, backslash, \\u)", () => {
    expect(shownSummary(node("string", spelling), true)).toBe(decoded);
    expect(shownSummary(node("string", '"\\u00e9"'), true)).toBe("é");
    expect(shownSummary(node("string", '"a\\\\b\\n\\""'), true)).toBe('a\\b\n"');
    expect(shownSummary(node("string", '""'), true)).toBe("");
  });

  it("leaves a non-string kind unchanged, even when the summary is a JSON string", () => {
    const kinds = [
      "object",
      "array",
      "number",
      "boolean",
      "null",
      "element",
      "text",
      "comment",
      "attribute",
    ] as const;
    for (const kind of kinds) {
      expect(shownSummary(node(kind, spelling), true), kind).toBe(spelling);
    }
    expect(shownSummary(node("number", "1.50e+3"), true)).toBe("1.50e+3");
    expect(shownSummary(node("boolean", "true"), true)).toBe("true");
    expect(shownSummary(node("null", "null"), true)).toBe("null");
    expect(shownSummary(node("text", '"a\\n"'), true)).toBe('"a\\n"');
  });

  it("leaves a non-JSON summary, and a JSON value that is not a string, unchanged", () => {
    for (const summary of ["", "not json", '"open', "1", "true", "false", "null", "[]", "{}"]) {
      expect(shownSummary(node("string", summary), true), JSON.stringify(summary)).toBe(summary);
    }
  });
});
