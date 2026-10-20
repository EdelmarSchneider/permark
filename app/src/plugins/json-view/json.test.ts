import { describe, expect, it } from "vitest";
import type { TreeNode } from "../../core/doc/tree";
import { parseJson } from "./json";

function tree(text: string): TreeNode {
  const parsed = parseJson(text);
  if (!parsed.ok) throw new Error(`not JSON at ${parsed.at}`);
  return parsed.root;
}

function plain(node: TreeNode): unknown {
  const kids = node.children?.() ?? [];
  if (node.kind === "object") return Object.fromEntries(kids.map((k) => [k.label, plain(k)]));
  if (node.kind === "array") return kids.map(plain);
  return node.summary;
}

describe("parseJson", () => {
  it("reads values as written", () => {
    expect(plain(tree('{"a": 1, "b": [true, false, null], "c": {"d": "x"}}'))).toEqual({
      a: "1",
      b: ["true", "false", "null"],
      c: { d: '"x"' },
    });
  });

  it("keeps the digits of numbers and the order and repeats of keys", () => {
    const root = tree('{"big": 12345678901234567890, "float": 1.50e+3, "b": 1, "a": 2, "b": 3}');
    const kids = root.children?.() ?? [];
    expect(kids.map((k) => `${k.label}=${k.summary}`)).toEqual([
      "big=12345678901234567890",
      "float=1.50e+3",
      "b=1",
      "a=2",
      "b=3",
    ]);
  });

  it("decodes strings, escapes and surrogate pairs", () => {
    const root = tree('"a\\n\\"\\u00e9\\ud83d\\ude00\\/"');
    expect(root.summary).toBe(JSON.stringify('a\n"é😀/'));
  });

  it("counts members", () => {
    expect(tree("{}").summary).toMatch(/0/);
    expect(tree("[1,2,3]").summary).toMatch(/3/);
    expect(tree("[]").children).toBeNull();
  });

  it("accepts whitespace around and a lone value", () => {
    expect(tree("  \n 42 \r\n").summary).toBe("42");
    expect(tree('"x"').kind).toBe("string");
  });

  it("refuses what is not JSON, saying where", () => {
    for (const bad of [
      "",
      "{",
      "[1,]",
      '{"a":1,}',
      "{'a':1}",
      '{"a" 1}',
      "[1 2]",
      "// c\n{}",
      "01",
      "1.",
      "-",
      '"\n"',
      '"\\x"',
      '"\\u12"',
      "{} {}",
      "nul",
      "[1]]",
    ]) {
      const parsed = parseJson(bad);
      expect(parsed.ok, bad).toBe(false);
    }
    const parsed = parseJson('{"a": tru}');
    expect(parsed.ok ? -1 : parsed.at).toBe(6);
  });

  it("survives deep nesting and big input without running out of stack", () => {
    expect(parseJson("[".repeat(100_000)).ok).toBe(false);
    expect(parseJson(`${"[".repeat(150)}${"]".repeat(150)}`).ok).toBe(true);
    const big = `[${Array.from({ length: 100_000 }, (_, i) => `{"i":${i},"s":"item ${i}"}`).join(",")}]`;
    const started = Date.now();
    const parsed = parseJson(big);
    expect(parsed.ok && (parsed.root.children?.() ?? []).length).toBe(100_000);
    // linear, not quadratic: about 1 s alone; the bound leaves room for a full parallel test run
    expect(Date.now() - started).toBeLessThan(8000);
    // the time bound above is the check; the runner's own limit must not cut in before it
  }, 20_000);
});
