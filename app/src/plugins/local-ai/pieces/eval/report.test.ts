import { describe, expect, it } from "vitest";
import type { Suggestion } from "../suggestion";
import {
  accepted,
  judged,
  runLine,
  scored,
  tallyTable,
  type EvalOutputs,
  type EvalRecord,
} from "./report";

function suggestion(from: number, to: number, insert: string): Suggestion {
  return { from, to, original: "", insert, before: "", after: "" };
}

const OUTPUTS: EvalOutputs = {
  profile: "gemma-4",
  model: "fixture.gguf",
  onGpu: true,
  loadMs: 12,
  totalMs: 340,
  tokPerSec: 41.26,
  records: [
    { id: "a", result: { kind: "output", text: "one" }, ms: 1, tokens: 1 },
    { id: "b", result: { kind: "discarded", reason: "tooLong" }, ms: 0, tokens: 0 },
  ],
};

describe("eval report of a piece job", () => {
  it("tells a runner discard from the plugin's verdict on an answer", () => {
    const [answer, dropped] = OUTPUTS.records;
    if (answer === undefined || dropped === undefined) throw new Error("fixture");
    const asked: string[] = [];
    const judge = (kind: "ok" | "same" | "discard") => (output: string) => {
      asked.push(output);
      if (kind === "discard") return { kind, reason: "script" };
      return kind === "ok" ? { kind, suggestions: [suggestion(0, 1, "x")] } : { kind };
    };
    expect(judged(dropped, judge("ok"))).toEqual({
      kind: "discard",
      reason: "tooLong",
      by: "runner",
    });
    expect(asked).toEqual([]);
    expect(judged(answer, judge("discard"))).toEqual({
      kind: "discard",
      reason: "script",
      by: "guards",
    });
    expect(judged(answer, judge("same"))).toEqual({ kind: "same" });
    expect(judged(answer, judge("ok"))).toEqual({
      kind: "ok",
      suggestions: [suggestion(0, 1, "x")],
    });
    expect(asked).toEqual(["one", "one", "one"]);
  });

  it("accepts every suggestion, whatever their order", () => {
    const doc = "one two three";
    expect(accepted(doc, [])).toBe(doc);
    expect(
      accepted(doc, [suggestion(0, 3, "1"), suggestion(8, 13, "3"), suggestion(4, 4, "and ")]),
    ).toBe("1 and two 3");
  });

  it("scores each case with its record, in case order, and refuses a file that is not this set's", () => {
    const cases = [{ id: "b" }, { id: "a" }];
    const kinds = (c: { id: string }, r: EvalRecord) => `${c.id}:${r.result.kind}`;
    expect(scored(cases, OUTPUTS, kinds)).toEqual(["b:discarded", "a:output"]);
    expect(() => scored([...cases, { id: "c" }], OUTPUTS, kinds)).toThrow("no output for case c");
    expect(() => scored([{ id: "a" }], OUTPUTS, kinds)).toThrow("outputs without a case: b");
    const twice = { ...OUTPUTS, records: [...OUTPUTS.records, ...OUTPUTS.records] };
    expect(() => scored(cases, twice, kinds)).toThrow("duplicate output ids");
  });

  it("prints the run in one line and a table per group with its total", () => {
    expect(runLine(OUTPUTS)).toBe(
      "model fixture.gguf | profile gemma-4 | gpu true | load 12 ms | generate 340 ms | 41.3 tok/s",
    );
    expect(runLine({ ...OUTPUTS, onGpu: null })).toContain("| gpu ? |");
    const list = [
      { lang: "en", pass: true },
      { lang: "ja", pass: false },
      { lang: "en", pass: false },
    ];
    const lines = tallyTable(
      list,
      { head: "lang", width: 6, key: (o) => o.lang },
      (group) => ({ cases: group.length, passed: group.filter((o) => o.pass).length }),
      [
        { head: "cases", width: 5, cell: (t) => String(t.cases) },
        { head: "pass", width: 6, cell: (t) => `${t.passed}/${t.cases}` },
      ],
    );
    expect(lines).toEqual([
      "lang   cases   pass",
      "en         2    1/2",
      "ja         1    0/1",
      "total      3    1/3",
    ]);
  });
});
