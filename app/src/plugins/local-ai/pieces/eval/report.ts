// Report half of a piece job's eval set (step 2 of 2). Step 1 (Rust, `local-ai`'s piece test kit) answers each case
// with a model and writes an outputs file; here each answer goes through the plugin's own checks (its `judge`, no
// copy of them) and the results are tallied into text tables. What a case is, what passing means and which columns a
// table shows stay with the plugin (`<plugin>/eval/report.ts`). A wrong outputs file (missing | extra | repeated
// records) throws; quality never does. Not shipped: imported by the plugins' eval reports, which only tests import.

import type { PieceRunResult } from "../../../../api/generated/PieceRunResult";
import type { PieceVerdict } from "../review.svelte";
import type { Suggestion } from "../suggestion";

/** The answer of one case in an outputs file (a plugin's records may hold more). */
export type EvalRecord = {
  id: string;
  result: PieceRunResult;
  ms: number;
  /** Answer tokens. */
  tokens: number;
};

/** An outputs file: the facts of the run + one record per case. */
export type EvalOutputs<R extends EvalRecord = EvalRecord> = {
  profile: string;
  model: string;
  /** `null`: no real model. */
  onGpu: boolean | null;
  loadMs: number;
  totalMs: number;
  tokPerSec: number;
  records: R[];
};

/** What became of one answer: suggestions, nothing to suggest, or dropped (`by` the runner before any check: answer
 *  cap, thinking, too long for the context; or by the plugin's guards). */
export type Judged =
  | { kind: "ok"; suggestions: Suggestion[] }
  | { kind: "same" }
  | { kind: "discard"; reason: string; by: "runner" | "guards" };

/** The verdict on `record`: the runner's own discard, else what the plugin's `judge` makes of the answer. */
export function judged(record: EvalRecord, judge: (output: string) => PieceVerdict): Judged {
  const { result } = record;
  if (result.kind === "discarded") return { kind: "discard", reason: result.reason, by: "runner" };
  const verdict = judge(result.text);
  return verdict.kind === "discard" ? { ...verdict, by: "guards" } : verdict;
}

/** `doc` after every one of `suggestions` is accepted. */
export function accepted(doc: string, suggestions: readonly Suggestion[]): string {
  let text = doc;
  for (const s of [...suggestions].sort((a, b) => b.from - a.from)) {
    text = text.slice(0, s.from) + s.insert + text.slice(s.to);
  }
  return text;
}

/** `score` of every case with its record, in case order; throws on missing | extra | duplicate records. */
export function scored<C extends { id: string }, R extends EvalRecord, O>(
  cases: readonly C[],
  outputs: EvalOutputs<R>,
  score: (c: C, record: R) => O,
): O[] {
  const byId = new Map(outputs.records.map((r) => [r.id, r]));
  if (byId.size !== outputs.records.length) throw new Error("duplicate output ids");
  const extra = outputs.records.filter((r) => !cases.some((c) => c.id === r.id));
  if (extra.length > 0)
    throw new Error(`outputs without a case: ${extra.map((r) => r.id).join(", ")}`);
  return cases.map((c) => {
    const record = byId.get(c.id);
    if (record === undefined) throw new Error(`no output for case ${c.id}`);
    return score(c, record);
  });
}

/** The facts of the run in one line. */
export function runLine(outputs: EvalOutputs): string {
  const gpu = outputs.onGpu === null ? "?" : String(outputs.onGpu);
  return `model ${outputs.model} | profile ${outputs.profile} | gpu ${gpu} | load ${outputs.loadMs} ms | generate ${outputs.totalMs} ms | ${outputs.tokPerSec.toFixed(1)} tok/s`;
}

/** A column of a tally table: its head, its width in chars, what it shows for the tally of a group. */
export type Column<T> = { head: string; width: number; cell: (tally: T) => string };

/** Lines of a text table over `list`: the heads, one row per group of `by.key` in order of first appearance, then
 *  the total; `tally` sums up a group, each column shows one fact of it. */
export function tallyTable<O, T>(
  list: readonly O[],
  by: { head: string; width: number; key: (o: O) => string },
  tally: (group: readonly O[]) => T,
  columns: readonly Column<T>[],
): string[] {
  const line = (name: string, cells: readonly string[]) =>
    [name.padEnd(by.width), ...cells.map((cell, i) => cell.padStart(columns[i]?.width ?? 0))].join(
      " ",
    );
  const row = (name: string, group: readonly O[]) => {
    const sum = tally(group);
    return line(
      name,
      columns.map((column) => column.cell(sum)),
    );
  };
  const names = [...new Set(list.map(by.key))];
  return [
    line(
      by.head,
      columns.map((column) => column.head),
    ),
    ...names.map((name) =>
      row(
        name,
        list.filter((o) => by.key(o) === name),
      ),
    ),
    row("total", list),
  ];
}
