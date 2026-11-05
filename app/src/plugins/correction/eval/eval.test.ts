// Eval set (spec feature-correction D-8) step 2: report over the outputs of step 1 (Rust
// `crates/plugins/correction/src/tests/eval.rs`). Synthetic report test always runs; the real report runs only when
// `PERMARK_EVAL_OUT` names an outputs file. Never fails on quality, only on harness errors.
//
// Run (cwd `app/`), PowerShell:
//   $env:PERMARK_EVAL_OUT="F:/tmp/eval-e2b.json"; npx vitest run src/plugins/correction/eval
// bash:
//   PERMARK_EVAL_OUT=/f/tmp/eval-e2b.json npx vitest run src/plugins/correction/eval

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import cases from "./cases.json";
import { outcomes, report, totals, type EvalCase, type EvalOutputs } from "./report";

const OUT = process.env["PERMARK_EVAL_OUT"];

const KINDS = new Set(["errors", "clean", "marker", "heading", "injection", "mixed"]);

function evalCases(): EvalCase[] {
  return cases.map((c) => {
    const kind = c.kind;
    if (!isKind(kind)) throw new Error(`${c.id}: unknown kind ${kind}`);
    return { ...c, kind };
  });
}

function isKind(kind: string): kind is EvalCase["kind"] {
  return KINDS.has(kind);
}

describe("correction eval report", () => {
  it("scores exact fixes, kept clean text, false discards and misses", () => {
    const fixture: EvalCase[] = [
      {
        id: "fix",
        lang: "en",
        kind: "marker",
        text: "Run ⟦1⟧ befor you push.",
        markers: { "⟦1⟧": "`npm test`" },
        expected: "Run ⟦1⟧ before you push.",
      },
      {
        id: "keep",
        lang: "en",
        kind: "clean",
        text: "The notes are ready.",
        expected: "The notes are ready.",
      },
      {
        id: "head",
        lang: "de",
        kind: "heading",
        text: "Neue einstellungen",
        expected: "Neue Einstellungen",
      },
      {
        id: "lost",
        lang: "de",
        kind: "errors",
        text: "Das ist ein dokument.",
        expected: "Das ist ein Dokument.",
      },
      {
        id: "wrong",
        lang: "pt-BR",
        kind: "errors",
        text: "O relatorio esta pronto.",
        expected: "O relatório está pronto.",
      },
    ];
    const outputs: EvalOutputs = {
      profile: "gemma-4",
      model: "fixture.gguf",
      onGpu: null,
      loadMs: 1,
      totalMs: 10,
      tokPerSec: 0,
      records: [
        {
          id: "fix",
          result: { kind: "output", text: "Run ⟦1⟧ before you push." },
          ms: 1,
          tokens: 6,
        },
        { id: "keep", result: { kind: "output", text: "The notes are ready." }, ms: 1, tokens: 5 },
        // final period the model added to a heading: dropped by the guards, rest kept
        { id: "head", result: { kind: "output", text: "Neue Einstellungen." }, ms: 1, tokens: 3 },
        { id: "lost", result: { kind: "discarded", reason: "length" }, ms: 1, tokens: 40 },
        {
          id: "wrong",
          result: { kind: "output", text: "O relatório esta pronto." },
          ms: 1,
          tokens: 5,
        },
      ],
    };
    const list = outcomes(fixture, outputs);
    expect(list.map((o) => [o.id, o.verdict, o.exact])).toEqual([
      ["fix", "ok", true],
      ["keep", "ok", true],
      ["head", "ok", true],
      ["lost", "length", false],
      ["wrong", "ok", false],
    ]);
    expect(list[0]?.result).toBe("Run `npm test` before you push.");
    expect(totals(list)).toEqual({
      cases: 5,
      exact: 3,
      clean: 1,
      cleanKept: 1,
      discarded: 1,
      falseDiscards: 1,
      missed: 1,
    });
    const text = report(fixture, outputs);
    expect(text).toContain("total      5     3/5     1/1     1      1      1");
    expect(text).toContain("- wrong [errors] ok:");
    expect(() => outcomes(fixture, { ...outputs, records: outputs.records.slice(1) })).toThrow(
      "no output for case fix",
    );
  });

  it("passes every expected answer of the eval set through the guards (oracle outputs score all exact)", () => {
    const list = evalCases();
    const oracle: EvalOutputs = {
      profile: "oracle",
      model: "oracle",
      onGpu: null,
      loadMs: 0,
      totalMs: 0,
      tokPerSec: 0,
      records: list.map((c) => ({
        id: c.id,
        result: { kind: "output", text: c.expected },
        ms: 0,
        tokens: 0,
      })),
    };
    const t = totals(outcomes(list, oracle));
    expect([t.cases, t.exact, t.discarded]).toEqual([34, 34, 0]);
  });

  it.skipIf(OUT === undefined)("reports the model outputs in PERMARK_EVAL_OUT", () => {
    const outputs: EvalOutputs = JSON.parse(readFileSync(OUT ?? "", "utf8"));
    console.log(report(evalCases(), outputs));
  });
});
