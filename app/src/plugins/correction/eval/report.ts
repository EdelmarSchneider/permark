// Eval set, step 2 of 2: scores the model outputs of step 1 (Rust `tests/eval.rs`) with the production pipeline
// (`restore` + `judge`, no copies), on the eval report of the shared pieces (`local-ai/pieces/eval/report.ts`: records
// paired with cases, verdicts, tables). Here: what a correction case is and what it scores. Per case: verdict (ok |
// discard reason, Rust or guards), text the person would get after accepting every suggestion, exact match vs
// expected. Report = table per language + totals + one line per non-exact case. Quality never fails a run; harness
// errors (missing | extra outputs, bad markers in a case) throw. Not shipped: imported by `eval.test.ts` only.

import {
  accepted,
  judged,
  runLine,
  scored,
  tallyTable,
  type Column,
  type EvalOutputs,
  type EvalRecord,
} from "../../local-ai/pieces/eval/report";
import { restore, type Piece } from "../../local-ai/pieces/segment";
import { judge } from "../guards";

export type { EvalOutputs, EvalRecord };

export type CaseKind = "errors" | "clean" | "marker" | "heading" | "injection" | "mixed";

export type EvalCase = {
  id: string;
  lang: string;
  kind: CaseKind;
  /** Piece text as sent to the model (non-prose as `⟦n⟧`). */
  text: string;
  /** Original bytes per marker (`"⟦1⟧": "`code`"`). */
  markers?: Record<string, string>;
  /** Expected answer, markers kept. */
  expected: string;
};

export type Outcome = {
  id: string;
  lang: string;
  kind: CaseKind;
  /** `ok` | discard reason (Rust: length, thinking, tooLong; guards: the rest). */
  verdict: string;
  /** Expected != input (the case has errors). */
  changed: boolean;
  /** Text after accepting every suggestion (discarded -> input unchanged). */
  result: string;
  /** Expected text, markers restored. */
  expected: string;
  exact: boolean;
};

export type Totals = {
  cases: number;
  exact: number;
  clean: number;
  cleanKept: number;
  discarded: number;
  /** Case with errors, answer discarded. */
  falseDiscards: number;
  /** Answer passed the guards, result != expected. */
  missed: number;
};

/** Piece of `c` as the window would have cut it (whole text = the document). */
export function pieceOf(c: EvalCase): Piece {
  const map = c.markers ?? {};
  const markers = Object.keys(map).map((_, i) => {
    const original = map[`⟦${i + 1}⟧`];
    if (original === undefined) throw new Error(`${c.id}: markers must be ⟦1⟧…⟦n⟧`);
    return original;
  });
  const shell: Piece = { from: 0, to: 0, kind: "paragraph", original: "", text: c.text, markers };
  const original = restore(c.text, shell);
  if (original === null) throw new Error(`${c.id}: text markers do not match its map`);
  const kind = c.kind === "heading" ? "heading" : "paragraph";
  return { ...shell, kind, original, to: original.length };
}

/** Score of one case. */
export function outcome(c: EvalCase, record: EvalRecord): Outcome {
  const piece = pieceOf(c);
  const expected = restore(c.expected, piece);
  if (expected === null) throw new Error(`${c.id}: expected markers do not match its map`);
  const verdict = judged(record, (output) => judge(piece.original, piece, output, c.lang));
  const result =
    verdict.kind === "ok" ? accepted(piece.original, verdict.suggestions) : piece.original;
  return {
    id: c.id,
    lang: c.lang,
    kind: c.kind,
    verdict: verdict.kind === "discard" ? verdict.reason : "ok",
    changed: c.expected !== c.text,
    result,
    expected,
    exact: result.normalize("NFC") === expected.normalize("NFC"),
  };
}

/** Every case scored, in case order; throws on missing | extra | duplicate outputs. */
export function outcomes(cases: readonly EvalCase[], outputs: EvalOutputs): Outcome[] {
  return scored(cases, outputs, outcome);
}

export function totals(list: readonly Outcome[]): Totals {
  const clean = list.filter((o) => !o.changed);
  const discarded = list.filter((o) => o.verdict !== "ok");
  return {
    cases: list.length,
    exact: list.filter((o) => o.exact).length,
    clean: clean.length,
    cleanKept: clean.filter((o) => o.result === o.expected).length,
    discarded: discarded.length,
    falseDiscards: discarded.filter((o) => o.changed).length,
    missed: list.filter((o) => o.verdict === "ok" && !o.exact).length,
  };
}

const COLUMNS: readonly Column<Totals>[] = [
  { head: "cases", width: 5, cell: (t) => String(t.cases) },
  { head: "exact", width: 7, cell: (t) => `${t.exact}/${t.cases}` },
  { head: "clean", width: 7, cell: (t) => `${t.cleanKept}/${t.clean}` },
  { head: "disc", width: 5, cell: (t) => String(t.discarded) },
  { head: "falseD", width: 6, cell: (t) => String(t.falseDiscards) },
  { head: "missed", width: 6, cell: (t) => String(t.missed) },
];

/** Compact text report: run facts, table per language + total, one line per non-exact case. */
export function report(cases: readonly EvalCase[], outputs: EvalOutputs): string {
  const list = outcomes(cases, outputs);
  const lines = [
    runLine(outputs),
    ...tallyTable(list, { head: "lang", width: 6, key: (o) => o.lang }, totals, COLUMNS),
  ];
  for (const o of list.filter((x) => !x.exact)) {
    lines.push(
      `- ${o.id} [${o.kind}] ${o.verdict}: got ${JSON.stringify(o.result)} want ${JSON.stringify(o.expected)}`,
    );
  }
  return lines.join("\n");
}
