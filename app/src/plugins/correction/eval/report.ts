// Eval set (spec feature-correction D-8) step 2: scores the model outputs of step 1 (Rust `tests/eval.rs`) with the
// production pipeline (`restore` + `judge`, no copies). Per case: verdict (ok | discard reason, Rust or guards), text
// the person would get after accepting every suggestion, exact match vs expected. Report = table per language +
// totals + one line per non-exact case. Quality never fails a run; harness errors (missing | extra outputs, bad
// markers in a case) throw. Not shipped: imported by `eval.test.ts` only.

import type { CorrectionPieceResult } from "../../../api/generated/CorrectionPieceResult";
import { judge } from "../guards";
import { restore, type Piece } from "../segment";

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

export type EvalRecord = { id: string; result: CorrectionPieceResult; ms: number; tokens: number };

export type EvalOutputs = {
  profile: string;
  model: string;
  onGpu: boolean | null;
  loadMs: number;
  totalMs: number;
  tokPerSec: number;
  records: EvalRecord[];
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
  const base = { id: c.id, lang: c.lang, kind: c.kind, changed: c.expected !== c.text, expected };
  const scored = (verdict: string, result: string): Outcome => ({
    ...base,
    verdict,
    result,
    exact: result.normalize("NFC") === expected.normalize("NFC"),
  });
  if (record.result.kind === "discarded") return scored(record.result.reason, piece.original);
  const verdict = judge(piece.original, piece, record.result.text, c.lang);
  if (verdict.kind === "discard") return scored(verdict.reason, piece.original);
  let result = piece.original;
  for (const s of [...verdict.suggestions].sort((a, b) => b.from - a.from)) {
    result = result.slice(0, s.from) + s.insert + result.slice(s.to);
  }
  return scored("ok", result);
}

/** Every case scored, in case order; throws on missing | extra | duplicate outputs. */
export function outcomes(cases: readonly EvalCase[], outputs: EvalOutputs): Outcome[] {
  const byId = new Map(outputs.records.map((r) => [r.id, r]));
  if (byId.size !== outputs.records.length) throw new Error("duplicate output ids");
  const extra = outputs.records.filter((r) => !cases.some((c) => c.id === r.id));
  if (extra.length > 0)
    throw new Error(`outputs without a case: ${extra.map((r) => r.id).join(", ")}`);
  return cases.map((c) => {
    const record = byId.get(c.id);
    if (record === undefined) throw new Error(`no output for case ${c.id}`);
    return outcome(c, record);
  });
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

/** Compact text report: run facts, table per language + total, one line per non-exact case. */
export function report(cases: readonly EvalCase[], outputs: EvalOutputs): string {
  const list = outcomes(cases, outputs);
  const langs = [...new Set(list.map((o) => o.lang))];
  const row = (name: string, t: Totals) =>
    [
      name.padEnd(6),
      String(t.cases).padStart(5),
      `${t.exact}/${t.cases}`.padStart(7),
      `${t.cleanKept}/${t.clean}`.padStart(7),
      String(t.discarded).padStart(5),
      String(t.falseDiscards).padStart(6),
      String(t.missed).padStart(6),
    ].join(" ");
  const gpu = outputs.onGpu === null ? "?" : String(outputs.onGpu);
  const lines = [
    `model ${outputs.model} | profile ${outputs.profile} | gpu ${gpu} | load ${outputs.loadMs} ms | generate ${outputs.totalMs} ms | ${outputs.tokPerSec.toFixed(1)} tok/s`,
    "lang   cases   exact   clean  disc falseD missed",
    ...langs.map((lang) => row(lang, totals(list.filter((o) => o.lang === lang)))),
    row("total", totals(list)),
  ];
  for (const o of list.filter((x) => !x.exact)) {
    lines.push(
      `- ${o.id} [${o.kind}] ${o.verdict}: got ${JSON.stringify(o.result)} want ${JSON.stringify(o.expected)}`,
    );
  }
  return lines.join("\n");
}
