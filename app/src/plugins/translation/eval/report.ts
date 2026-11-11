// Eval set, step 2 of 2: scores the model outputs of step 1 (Rust `tests/eval.rs`) with the production pipeline (the
// app's segmenter + `restore` + this plugin's `judge` with its own limits: no copies), on the eval report of the
// shared pieces (`local-ai/pieces/eval/report.ts`: records paired with cases, verdicts, tables). Here: what a
// translation case is and what it measures. No reference translation: a case says what a machine can check (a
// translation offered | nothing to suggest, strings that must stay, strings that must not come), the rest is for a
// person to read.
//
// Report: run facts + prompt variant; tables per pair and per kind; rates (structure, wrong script, expectations);
// discard reasons; one line per unmet expectation and per kept | banned string; the answers the guards dropped that
// read like translations (false-discard candidates, for a person to label); the translations into `READ` in full.
// Quality never fails a run; harness errors (missing | extra outputs, a case whose piece is not in its block) throw.
// Not shipped: imported by `eval.test.ts` only.

import {
  accepted,
  judged,
  runLine,
  scored,
  tallyTable,
  type Column,
  type EvalOutputs as RunOutputs,
  type EvalRecord as RunRecord,
} from "../../local-ai/pieces/eval/report";
import { pieces, restore, unmarked, type Piece } from "../../local-ai/pieces/segment";
import type { Suggestion } from "../../local-ai/pieces/suggestion";
import { judge } from "../guards";

const KINDS = [
  "plain",
  "markers",
  "heading",
  "listItem",
  "cell",
  "footnote",
  "numbers",
  "identifiers",
  "request",
  "same",
  "mixed",
  "long",
  "alert",
  "emphasis",
] as const;

export type CaseKind = (typeof KINDS)[number];

/** What a case's answer must come to: a translation offered | nothing to suggest (the piece is in the target). */
export type Expect = "translated" | "same";

export type EvalCase = {
  id: string;
  kind: CaseKind;
  /** Language of the document the piece comes from (with `target`: the pair of the tables); the piece itself may
   *  be in another one (kinds `same`, `mixed`). */
  from: string;
  /** Source the run names; `null`: read off the piece. */
  source: string | null;
  target: string;
  /** Markdown block the piece is cut from. */
  markdown: string;
  /** Piece text as sent to the model (non-prose as `⟦n⟧`): what the segmenter makes of one prose block of
   *  `markdown`. */
  text: string;
  expect: Expect;
  /** Must stand unchanged in the answer (names, numbers, identifiers). */
  mustKeep?: string[];
  /** Must not stand in the answer, whatever the letter case (an answer to a question, words of another language). */
  mustNotContain?: string[];
  /** For the person reading the report. */
  note?: string;
};

/** A case as the cases file holds it. */
export type RawCase = Omit<EvalCase, "kind" | "expect"> & { kind: string; expect: string };

/** Prompt of a run: the app's | the same without its example turns. */
export type Variant = "fewShot" | "zeroShot";

export type EvalRecord = RunRecord & {
  kind: string;
  from: string;
  source: string | null;
  target: string;
  /** Tokens of the whole prompt, as the engine counted them. */
  promptTokens: number;
  /** Answer cap asked. */
  maxTokens: number;
};

export type EvalOutputs = RunOutputs<EvalRecord> & { variant: Variant };

export type Outcome = {
  id: string;
  kind: CaseKind;
  /** `<from>-><target>`. */
  pair: string;
  target: string;
  /** `ok` | `same` | the guards' discard reason | `runner:<reason>` (dropped before any guard: answer cap,
   *  thinking, too long for the context). */
  verdict: string;
  /** The piece as written in its block. */
  source: string;
  /** What the model said, as far as it can be read: the translation offered, else its answer with the markers back
   *  where they can go back; `null`: the runner dropped it. */
  answer: string | null;
  /** What the person would be offered in the piece's place. */
  translation: string | null;
  /** The case's `expect` holds. */
  met: boolean;
  /** `mustKeep` strings the answer lost. */
  lost: string[];
  /** `mustNotContain` strings the answer holds. */
  banned: string[];
  /** Dropped by the guards although the answer reads like a translation: for a person to label. */
  candidate: boolean;
  note: string | undefined;
  ms: number;
  tokens: number;
  promptTokens: number;
};

export type Tally = {
  cases: number;
  /** Answers that reached the guards. */
  answered: number;
  ok: number;
  same: number;
  discarded: number;
  met: number;
  /** Answers dropped for the Markdown of the piece (`STRUCTURE`). */
  structure: number;
  /** Answers dropped for the script of the target. */
  script: number;
  /** Cases whose answer lost a string to keep | holds a banned one. */
  lost: number;
  banned: number;
  candidates: number;
  tokens: number;
  ms: number;
  promptTokens: number;
};

/** Guard reasons that say an answer broke the Markdown of its piece (markers, symbol counts, Markdown it does not
 *  have, a table row, the lines a marker holds). The first failing check names an answer: one dropped earlier
 *  (thinking, an announced answer) is not counted here. */
export const STRUCTURE: readonly string[] = ["markers", "symbols", "added", "cell", "lines"];
/** Target whose translations the report prints in full (read by a person). */
export const READ = "pt-BR";

const LETTER = /\p{L}/u;

function isKind(kind: string): kind is CaseKind {
  return KINDS.some((k) => k === kind);
}

/** The cases of the cases file, typed; throws on a kind | an expectation the report does not know. */
export function evalCases(raw: readonly RawCase[]): EvalCase[] {
  return raw.map((c) => {
    const { kind, expect } = c;
    if (!isKind(kind)) throw new Error(`${c.id}: unknown kind ${kind}`);
    if (expect !== "translated" && expect !== "same")
      throw new Error(`${c.id}: unknown expect ${expect}`);
    return { ...c, kind, expect };
  });
}

/** The piece of `c` as the app's segmenter cuts it from the case's block: the one that reads as the case's text. */
export function pieceOf(c: EvalCase): Piece {
  const [piece, ...more] = pieces(c.markdown).filter((p) => p.text === c.text);
  if (piece === undefined || more.length > 0)
    throw new Error(`${c.id}: its text is not one piece of its block`);
  return piece;
}

// the text in the place of `piece` after `suggestions` (ranges inside the piece, in offsets of `doc`) are accepted
function pieceAfter(doc: string, piece: Piece, suggestions: readonly Suggestion[]): string {
  const after = accepted(doc, suggestions);
  return after.slice(piece.from, after.length - (doc.length - piece.to));
}

// what two texts are compared by: NFC, any run of whitespace as one space, none around
function plain(text: string): string {
  return text.normalize("NFC").replace(/\s+/gu, " ").trim();
}

/** Score of one case. */
export function outcome(c: EvalCase, record: EvalRecord): Outcome {
  const piece = pieceOf(c);
  const verdict = judged(record, (output) =>
    judge(c.markdown, piece, output, { target: c.target, source: c.source }),
  );
  const raw = record.result.kind === "output" ? record.result.text.trim() : null;
  const translation =
    verdict.kind === "ok" ? pieceAfter(c.markdown, piece, verdict.suggestions) : null;
  const answer = translation ?? (raw === null ? null : (restore(raw, piece, "anyOrder") ?? raw));
  const said = answer?.toLowerCase() ?? "";
  const lost = answer === null ? [] : (c.mustKeep ?? []).filter((k) => !answer.includes(k));
  const banned = (c.mustNotContain ?? []).filter((b) => said.includes(b.toLowerCase()));
  return {
    id: c.id,
    kind: c.kind,
    pair: `${c.from}->${c.target}`,
    target: c.target,
    verdict:
      verdict.kind !== "discard"
        ? verdict.kind
        : verdict.by === "runner"
          ? `runner:${verdict.reason}`
          : verdict.reason,
    source: piece.original,
    answer,
    translation,
    met: verdict.kind === (c.expect === "translated" ? "ok" : "same"),
    lost,
    banned,
    candidate:
      verdict.kind === "discard" &&
      verdict.by === "guards" &&
      raw !== null &&
      LETTER.test(unmarked(raw)) &&
      plain(answer ?? "") !== plain(piece.original) &&
      banned.length === 0,
    note: c.note,
    ms: record.ms,
    tokens: record.tokens,
    promptTokens: record.promptTokens,
  };
}

/** Every case scored, in case order; throws on missing | extra | duplicate outputs. */
export function outcomes(cases: readonly EvalCase[], outputs: EvalOutputs): Outcome[] {
  return scored(cases, outputs, outcome);
}

export function tally(list: readonly Outcome[]): Tally {
  const count = (test: (o: Outcome) => boolean) => list.filter(test).length;
  const sum = (of: (o: Outcome) => number) => list.reduce((total, o) => total + of(o), 0);
  const answered = count((o) => !o.verdict.startsWith("runner:"));
  const ok = count((o) => o.verdict === "ok");
  const same = count((o) => o.verdict === "same");
  return {
    cases: list.length,
    answered,
    ok,
    same,
    discarded: list.length - ok - same,
    met: count((o) => o.met),
    structure: count((o) => STRUCTURE.includes(o.verdict)),
    script: count((o) => o.verdict === "script"),
    lost: count((o) => o.lost.length > 0),
    banned: count((o) => o.banned.length > 0),
    candidates: count((o) => o.candidate),
    tokens: sum((o) => o.tokens),
    ms: sum((o) => o.ms),
    promptTokens: sum((o) => o.promptTokens),
  };
}

/** Discards by reason, most frequent first (then by name). */
export function discards(list: readonly Outcome[]): [reason: string, count: number][] {
  const counts = new Map<string, number>();
  for (const o of list) {
    if (o.verdict === "ok" || o.verdict === "same") continue;
    counts.set(o.verdict, (counts.get(o.verdict) ?? 0) + 1);
  }
  return [...counts].sort(([a, n], [b, m]) => m - n || a.localeCompare(b));
}

const COLUMNS: readonly Column<Tally>[] = [
  { head: "cases", width: 5, cell: (t) => String(t.cases) },
  { head: "met", width: 6, cell: (t) => `${t.met}/${t.cases}` },
  { head: "ok", width: 3, cell: (t) => String(t.ok) },
  { head: "same", width: 4, cell: (t) => String(t.same) },
  { head: "disc", width: 4, cell: (t) => String(t.discarded) },
  { head: "struct", width: 6, cell: (t) => String(t.structure) },
  { head: "script", width: 6, cell: (t) => String(t.script) },
  { head: "lost", width: 4, cell: (t) => String(t.lost) },
  { head: "banned", width: 6, cell: (t) => String(t.banned) },
  { head: "falseD?", width: 7, cell: (t) => String(t.candidates) },
  {
    head: "tok/s",
    width: 6,
    cell: (t) => (t.ms > 0 ? ((1000 * t.tokens) / t.ms).toFixed(1) : "-"),
  },
];

function share(n: number, of: number): string {
  return of > 0 ? `${n}/${of} (${((100 * n) / of).toFixed(0)}%)` : `${n}/${of}`;
}

// `text` under a label, its further lines aligned with the first
function labelled(label: string, text: string): string {
  return label + text.split("\n").join(`\n${" ".repeat(label.length)}`);
}

/** The translations into `target`, in full: per case its piece, then what the person would be offered (or why
 *  nothing is). */
export function reading(list: readonly Outcome[], target: string): string[] {
  return list
    .filter((o) => o.target === target)
    .flatMap((o) => [
      `[${o.id}] ${o.kind}`,
      labelled("  ", o.source),
      labelled("  -> ", o.translation ?? `(${o.verdict})`),
    ]);
}

/** Text report (module comment: what it holds). */
export function report(cases: readonly EvalCase[], outputs: EvalOutputs): string {
  const list = outcomes(cases, outputs);
  const total = tally(list);
  const one = (o: Outcome) => `- ${o.id} [${o.kind}] ${o.verdict}`;
  const reasons = discards(list).map(([reason, count]) => `${reason} ${count}`);
  const unmet = list.filter((o) => !o.met);
  const strings = list.filter((o) => o.lost.length + o.banned.length > 0);
  const offered = strings.filter((o) => o.translation !== null).length;
  const candidates = list.filter((o) => o.candidate);
  return [
    `variant ${outputs.variant} | ${runLine(outputs)}`,
    "",
    ...tallyTable(list, { head: "pair", width: 13, key: (o) => o.pair }, tally, COLUMNS),
    "",
    ...tallyTable(list, { head: "kind", width: 13, key: (o) => o.kind }, tally, COLUMNS),
    "",
    `structure pass ${share(total.answered - total.structure, total.answered)} | wrong script ${share(total.script, total.answered)} | expectation met ${share(total.met, total.cases)} | prompt ${Math.round(total.promptTokens / Math.max(1, total.cases))} tok/case`,
    `discards (${total.discarded}): ${reasons.join(", ") || "none"}`,
    "",
    `expectation not met (${unmet.length}):`,
    ...unmet.map((o) => `${one(o)}: ${JSON.stringify(o.answer)}`),
    "",
    `strings lost | banned (${strings.length} cases, ${offered} of them offered to the person):`,
    ...strings.map(
      (o) => `${one(o)}: lost ${JSON.stringify(o.lost)} banned ${JSON.stringify(o.banned)}`,
    ),
    "",
    `false-discard candidates (${candidates.length} of ${total.discarded} discards; label each: translation | not):`,
    ...candidates.flatMap((o) => [
      one(o) + (o.note === undefined ? "" : ` (${o.note})`),
      labelled("    source: ", o.source),
      labelled("    answer: ", o.answer ?? ""),
    ]),
    "",
    `translations into ${READ}:`,
    ...reading(list, READ),
  ].join("\n");
}
