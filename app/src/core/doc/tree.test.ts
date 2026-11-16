import { describe, expect, it } from "vitest";
import {
  CHUNK,
  EXPAND_BUDGET,
  OPEN_BUDGET,
  nextAsk,
  openDepth,
  shownSummary,
  spreadOf,
  type TreeNode,
} from "./tree";

function node(kind: TreeNode["kind"], summary: string): TreeNode {
  return { label: null, kind, summary, children: null };
}

const decoded = 'a\n\t\r"\\\u0001';
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

/** A node with `width` children per level, `levels` levels below it; leaves at the bottom. */
function wide(width: number, levels: number, label: string | null = null): TreeNode {
  if (levels === 0) return { label, kind: "number", summary: "1", children: null };
  return {
    label,
    kind: "array",
    summary: `${width} items`,
    children: () => Array.from({ length: width }, (_, at) => wide(width, levels - 1, String(at))),
  };
}

describe("openDepth", () => {
  it("opens everything when the whole tree fits the budget", () => {
    expect(openDepth(wide(3, 3), 100)).toBe(Infinity);
    expect(openDepth(wide(0, 1), 1)).toBe(Infinity);
    expect(openDepth({ label: null, kind: "null", summary: "null", children: null }, 1)).toBe(
      Infinity,
    );
  });

  it("opens the full levels that fit, and at least the root", () => {
    // rows by level: 1, 10, 100, 1000
    expect(openDepth(wide(10, 3), 111)).toBe(2);
    expect(openDepth(wide(10, 3), 1110)).toBe(2);
    expect(openDepth(wide(10, 3), 1111)).toBe(Infinity);
    expect(openDepth(wide(10, 3), 50)).toBe(1);
    expect(openDepth(wide(10, 3), 5)).toBe(1);
  });

  it("counts only the children a row shows at first (one chunk)", () => {
    // 5000 leaves under the root: one chunk of them is drawn
    expect(openDepth(wide(5000, 1), CHUNK + 1)).toBe(Infinity);
    expect(openDepth(wide(5000, 1), CHUNK)).toBe(1);
  });

  it("does not make children of rows below the levels it counted", () => {
    let made = 0;
    const counted = (levels: number): TreeNode => ({
      label: null,
      kind: "array",
      summary: "",
      children: () => {
        made += 1;
        return levels === 0 ? [] : Array.from({ length: 10 }, () => counted(levels - 1));
      },
    });
    openDepth(counted(6), 50);
    expect(made).toBeLessThanOrEqual(11);
  });
});

describe("spreadOf", () => {
  it("opens a small document fully and a big one at its first level", () => {
    expect(spreadOf(wide(3, 3), null)).toEqual({ depth: Infinity, at: 0 });
    expect(spreadOf(wide(CHUNK, 3), null).depth).toBeLessThan(Infinity);
    expect(OPEN_BUDGET).toBeLessThan(EXPAND_BUDGET);
  });

  it("follows the last ask: expand opens all that fits the larger budget, collapse leaves the root open", () => {
    const big = wide(CHUNK, 3);
    const expand = nextAsk(null, true);
    const collapse = nextAsk(expand, false);
    expect(expand.at).toBe(1);
    expect(collapse.at).toBe(2);
    expect(spreadOf(big, expand)).toEqual({ depth: openDepth(big, EXPAND_BUDGET), at: 1 });
    expect(spreadOf(big, collapse)).toEqual({ depth: 1, at: 2 });
    expect(spreadOf(wide(3, 3), expand)).toEqual({ depth: Infinity, at: 1 });
    expect(nextAsk(nextAsk(collapse, true), true).at).toBe(4);
  });
});
