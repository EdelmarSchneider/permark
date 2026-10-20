import { readFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";
import { describe, expect, it } from "vitest";
import { chars4, classOf, estimate, generic } from "./estimate";
import type { TokenizerId } from "./tokenizers";

type Fixtures = {
  cases: { name: string; text: string; estimates: Record<string, number> }[];
};

// the same file the Rust crate checks: the three copies of the model (Python tool, Rust, this one) agree
const fixtures = JSON.parse(
  readFileSync(
    join(process.cwd(), "src-tauri/crates/plugins/token-count/fixtures/estimate.json"),
    "utf8",
  ),
) as Fixtures;

const FAMILIES: [TokenizerId, string][] = [
  ["o200k", "o200k"],
  ["cl100k", "cl100k"],
  ["gemini", "gemini"],
  ["claude", "claude"],
  ["chars4", "chars4"],
];

describe("estimates (mirror of the Rust crate)", () => {
  it("gives the shared fixtures' numbers for every family", () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(50);
    for (const item of fixtures.cases) {
      expect(generic(item.text), `${item.name} (generic)`).toBe(item.estimates["generic"]);
      for (const [id, family] of FAMILIES) {
        expect(estimate(id, item.text), `${item.name} (${family})`).toBe(item.estimates[family]);
      }
    }
  });

  it("puts characters in classes by code point range", () => {
    expect(classOf(0x20)).toBe("space");
    expect(classOf(0x0a)).toBe("newline");
    expect(classOf(0x37)).toBe("digit");
    expect(classOf(0x51)).toBe("upper");
    expect(classOf(0x71)).toBe("lower");
    expect(classOf(0x21)).toBe("punct");
    expect(classOf(0xa9)).toBe("symbol");
    expect(classOf(0xe9)).toBe("latinExt");
    expect(classOf(0x4f60)).toBe("cjk");
    expect(classOf(0x416)).toBe("greekCyr");
    expect(classOf(0x627)).toBe("letterOther");
    expect(classOf(0x1f600)).toBe("emoji");
    expect(classOf(0x2192)).toBe("symbol");
  });

  it("counts the rule of thumb in scalar values, not UTF-16 units", () => {
    expect(chars4("")).toBe(0);
    expect(chars4("abcde")).toBe(2);
    expect(chars4("😀😀😀😀")).toBe(1);
  });

  it("never gives a negative or fractional number, and grows with the text", () => {
    for (const [id] of FAMILIES) {
      expect(estimate(id, "")).toBe(0);
      const small = estimate(id, "one two three");
      const large = estimate(id, "one two three ".repeat(50));
      expect(Number.isInteger(small) && small > 0).toBe(true);
      expect(large).toBeGreaterThan(small);
    }
  });
});
