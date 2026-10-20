import { readFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";
import { describe, expect, it } from "vitest";
import { measure, minutes } from "./count";

type Case = {
  name: string;
  kind: "markdown" | "plain";
  text: string;
  words: number;
  characters: number;
  minutes: number;
};

const cases = JSON.parse(
  readFileSync(join(process.cwd(), "src-tauri/crates/plugins/word-count/fixtures.json"), "utf8"),
) as Case[];

describe("word count (mirror of the Rust crate)", () => {
  it("counts every shared fixture like the backend", () => {
    expect(cases.length).toBeGreaterThanOrEqual(15);
    for (const c of cases) {
      const counts = measure(c.text, c.kind);
      expect(counts.words, `${c.name}: words`).toBe(c.words);
      expect(counts.characters, `${c.name}: characters`).toBe(c.characters);
      expect(minutes(counts), `${c.name}: minutes`).toBe(c.minutes);
    }
  });

  it("counts a long text quickly", () => {
    const text = "Some words, a [link](x.md) and `code`.\n".repeat(30_000);
    const started = performance.now();
    expect(measure(text, "markdown").words).toBe(150_000);
    expect(performance.now() - started).toBeLessThan(3000);
  });
});
