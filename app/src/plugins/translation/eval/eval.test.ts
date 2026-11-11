// Eval set, step 2 of 2: report over the outputs of step 1 (Rust
// `crates/plugins/translation/src/tests/eval.rs`). Always run: the cases file against the app's segmenter and
// language list, the report over a fixture and over what the fake engine answers. The real report runs only when
// `PERMARK_EVAL_OUT` names an outputs file. Never fails on quality, only on harness errors.
//
// Run (cwd `app/`; `--silent=false`: the report is the console output of a passing test, which vitest hides when a
// tool runs it), PowerShell:
//   $env:PERMARK_EVAL_OUT="F:/tmp/translation-e2b-fewShot.json"
//   npx vitest run src/plugins/translation/eval --silent=false
// bash:
//   PERMARK_EVAL_OUT=/f/tmp/translation-e2b-fewShot.json npx vitest run src/plugins/translation/eval --silent=false

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { englishName, offered } from "../languages";
import set from "./cases.json";
import {
  discards,
  evalCases,
  outcomes,
  pieceOf,
  READ,
  report,
  tally,
  type EvalCase,
  type EvalOutputs,
  type EvalRecord,
} from "./report";

const OUT = process.env["PERMARK_EVAL_OUT"];

const RUN = { profile: "fake", model: "fake", onGpu: null, loadMs: 0, totalMs: 0, tokPerSec: 0 };

function record(c: EvalCase, result: EvalRecord["result"]): EvalRecord {
  const { id, kind, from, source, target } = c;
  return {
    id,
    kind,
    from,
    source,
    target,
    result,
    ms: 0,
    tokens: 0,
    promptTokens: 0,
    maxTokens: 0,
  };
}

function said(c: EvalCase, text: string): EvalRecord {
  return record(c, { kind: "output", text });
}

/** The request as the prompt words it: what the fake engine echoes, word by word, before the piece. */
function request(c: EvalCase): string {
  const named = (tag: string) => `${englishName(tag)} (${tag})`;
  const source = c.source === null ? "the language it is written in" : named(c.source);
  return `Translate from ${source} to ${named(c.target)}:`;
}

describe("translation eval set", () => {
  const cases = evalCases(set.cases);

  it("holds for every case the piece the segmenter cuts from its block", () => {
    const kinds = new Map(cases.map((c) => [c.id, pieceOf(c).kind]));
    expect(kinds.size).toBe(cases.length);
    // the Markdown around a piece is its block's, not the piece's
    for (const c of cases) {
      const piece = pieceOf(c);
      expect(c.markdown.slice(piece.from, piece.to), c.id).toBe(piece.original);
      expect(piece.markers.length, c.id).toBe(c.text.split("⟦").length - 1);
    }
    expect(kinds.get("en-ptbr-heading")).toBe("heading");
    expect(kinds.get("en-ptbr-task")).toBe("listItem");
    expect(kinds.get("en-de-list")).toBe("listItem");
    expect(kinds.get("en-ptbr-cell")).toBe("cell");
    expect(kinds.get("en-zh-cell")).toBe("cell");
    expect(kinds.get("en-ptbr-alert")).toBe("quote");
    expect(kinds.get("en-de-alert")).toBe("quote");
    // an alert's marker stays out; the quote mark of its second line is a marker
    const alert = cases.find((c) => c.id === "en-de-alert");
    expect(alert && pieceOf(alert).markers).toEqual([">"]);
    const footnote = cases.find((c) => c.id === "en-ptbr-footnote");
    expect(footnote && pieceOf(footnote).markers).toEqual(["[^1]"]);
  });

  it("names the languages as the app does: tags it offers, their English names for the prompt", () => {
    const sent = new Set(
      cases.flatMap((c) => (c.source === null ? [c.target] : [c.source, c.target])),
    );
    expect([...sent].sort()).toEqual(Object.keys(set.languages).sort());
    for (const [tag, name] of Object.entries(set.languages)) {
      expect(offered(), tag).toContain(tag);
      expect(englishName(tag), tag).toBe(name);
    }
  });
});

describe("translation eval report", () => {
  const fixture: EvalCase[] = evalCases([
    {
      id: "moved",
      kind: "markers",
      from: "en",
      source: null,
      target: "de",
      markdown: "Open `notes.md` in the editor and read the [guide](guide.md) first.",
      text: "Open ⟦1⟧ in the editor and read the [guide]⟦2⟧ first.",
      expect: "translated",
      mustKeep: ["notes.md"],
    },
    {
      id: "already",
      kind: "same",
      from: "en",
      source: null,
      target: "pt-BR",
      markdown: "As notas ficam nesta pasta.",
      text: "As notas ficam nesta pasta.",
      expect: "same",
    },
    {
      id: "echoed",
      kind: "plain",
      from: "en",
      source: null,
      target: "pt-BR",
      markdown: "The notes stay in this folder.",
      text: "The notes stay in this folder.",
      expect: "translated",
    },
    {
      id: "capped",
      kind: "long",
      from: "en",
      source: null,
      target: "pt-BR",
      markdown: "Every change is saved as you type, and nothing leaves this computer.",
      text: "Every change is saved as you type, and nothing leaves this computer.",
      expect: "translated",
    },
    {
      id: "names",
      kind: "mixed",
      from: "en",
      source: null,
      target: "ja",
      markdown: "Open PerMark and pick Local AI in the settings.",
      text: "Open PerMark and pick Local AI in the settings.",
      expect: "translated",
      mustKeep: ["PerMark", "Local AI"],
      note: "names in Latin letters",
    },
    {
      id: "answered",
      kind: "request",
      from: "en",
      source: null,
      target: "pt-BR",
      markdown: "What does the abbreviation HTML stand for?",
      text: "What does the abbreviation HTML stand for?",
      expect: "translated",
      mustKeep: ["HTML", "?"],
      mustNotContain: ["hypertext"],
    },
    {
      id: "dropped",
      kind: "markers",
      from: "en",
      source: null,
      target: "pt-BR",
      markdown: "Run `sync --all` once a day.",
      text: "Run ⟦1⟧ once a day.",
      expect: "translated",
    },
  ]);
  const at = (id: string): EvalCase => {
    const found = fixture.find((c) => c.id === id);
    if (found === undefined) throw new Error(id);
    return found;
  };
  const outputs: EvalOutputs = {
    ...RUN,
    variant: "zeroShot",
    records: [
      // German word order: the markers move, each still once
      said(at("moved"), "Lies zuerst den [Leitfaden]⟦2⟧ und öffne dann ⟦1⟧ im Editor."),
      said(at("already"), "As notas ficam nesta pasta."),
      said(at("echoed"), "The notes stay in this folder."),
      record(at("capped"), { kind: "discarded", reason: "length" }),
      // a fine translation: its names in Latin letters outweigh the Japanese around them
      said(at("names"), "PerMark を開き、設定で Local AI を選びます。"),
      // an answer to the question, in the target's script and of a likely length: no guard tells it
      said(at("answered"), "HTML significa HyperText Markup Language."),
      said(at("dropped"), "Execute a sincronização uma vez por dia."),
    ],
  };

  it("scores expectations, structure, script, kept and banned strings, false-discard candidates", () => {
    const list = outcomes(fixture, outputs);
    expect(list.map((o) => [o.id, o.pair, o.verdict, o.met])).toEqual([
      ["moved", "en->de", "ok", true],
      ["already", "en->pt-BR", "same", true],
      ["echoed", "en->pt-BR", "same", false],
      ["capped", "en->pt-BR", "runner:length", false],
      ["names", "en->ja", "script", false],
      ["answered", "en->pt-BR", "ok", true],
      ["dropped", "en->pt-BR", "markers", false],
    ]);
    const [moved, , , capped, names, answered, dropped] = list;
    expect(moved?.translation).toBe(
      "Lies zuerst den [Leitfaden](guide.md) und öffne dann `notes.md` im Editor.",
    );
    expect([moved?.lost, moved?.banned]).toEqual([[], []]);
    expect([capped?.answer, capped?.candidate]).toEqual([null, false]);
    expect([names?.candidate, names?.lost, names?.translation]).toEqual([true, [], null]);
    expect([answered?.lost, answered?.banned]).toEqual([["?"], ["hypertext"]]);
    expect([dropped?.candidate, dropped?.answer]).toEqual([
      true,
      "Execute a sincronização uma vez por dia.",
    ]);
    expect(tally(list)).toEqual({
      cases: 7,
      answered: 6,
      ok: 2,
      same: 2,
      discarded: 3,
      met: 3,
      structure: 1,
      script: 1,
      lost: 1,
      banned: 1,
      candidates: 2,
      tokens: 0,
      ms: 0,
      promptTokens: 0,
    });
    expect(discards(list)).toEqual([
      ["markers", 1],
      ["runner:length", 1],
      ["script", 1],
    ]);
  });

  it("prints the run, the tables, the rates, what a person must look at and the translations to read", () => {
    const text = report(fixture, outputs);
    expect(text).toContain("variant zeroShot | model fake | profile fake | gpu ?");
    expect(text).toContain(
      "pair          cases    met  ok same disc struct script lost banned falseD?  tok/s",
    );
    expect(text).toContain(
      "en->pt-BR         5    2/5   1    2    2      1      0    1      1       1      -",
    );
    expect(text).toContain(
      "total             7    3/7   2    2    3      1      1    1      1       2      -",
    );
    expect(text).toContain("kind          cases    met");
    expect(text).toContain(
      "markers           2    1/2   1    0    1      1      0    0      0       1      -",
    );
    expect(text).toContain(
      "structure pass 5/6 (83%) | wrong script 1/6 (17%) | expectation met 3/7 (43%) | prompt 0 tok/case",
    );
    expect(text).toContain("discards (3): markers 1, runner:length 1, script 1");
    expect(text).toContain('- echoed [plain] same: "The notes stay in this folder."');
    expect(text).toContain("- capped [long] runner:length: null");
    expect(text).toContain("strings lost | banned (1 cases, 1 of them offered to the person):");
    expect(text).toContain('- answered [request] ok: lost ["?"] banned ["hypertext"]');
    expect(text).toContain(
      "false-discard candidates (2 of 3 discards; label each: translation | not):",
    );
    expect(text).toContain(
      [
        "- names [mixed] script (names in Latin letters)",
        "    source: Open PerMark and pick Local AI in the settings.",
        "    answer: PerMark を開き、設定で Local AI を選びます。",
      ].join("\n"),
    );
    expect(text).toContain(
      [
        `translations into ${READ}:`,
        "[already] same",
        "  As notas ficam nesta pasta.",
        "  -> (same)",
        "[echoed] plain",
        "  The notes stay in this folder.",
        "  -> (same)",
        "[capped] long",
        "  Every change is saved as you type, and nothing leaves this computer.",
        "  -> (runner:length)",
        "[answered] request",
        "  What does the abbreviation HTML stand for?",
        "  -> HTML significa HyperText Markup Language.",
        "[dropped] markers",
        "  Run `sync --all` once a day.",
        "  -> (markers)",
      ].join("\n"),
    );
    expect(() => outcomes(fixture, { ...outputs, records: outputs.records.slice(1) })).toThrow(
      "no output for case moved",
    );
  });

  it("runs over what the fake engine answers for every case of the set (the request echoed before the piece)", () => {
    const cases = evalCases(set.cases);
    const echoed: EvalOutputs = {
      ...RUN,
      variant: "fewShot",
      records: cases.map((c) => said(c, `${request(c)} ${c.text}`.split(/\s+/).join(" "))),
    };
    const list = outcomes(cases, echoed);
    const total = tally(list);
    expect([total.cases, total.answered]).toEqual([cases.length, cases.length]);
    // never the bare piece: nothing reads as already in the target
    expect(total.same).toBe(0);
    expect(list.filter((o) => o.kind === "same").some((o) => o.met)).toBe(false);
    // an echo holds the words of its source: banned where the case bans those (Spanish left in Portuguese)
    const banned = list.filter((o) => o.banned.length > 0);
    expect(banned.map((o) => o.pair)).toEqual(["es->pt-BR", "es->pt-BR", "es->pt-BR"]);
    // an echo of English is not in the script of Japanese, Chinese or Arabic
    for (const o of list.filter(
      (x) => x.pair.startsWith("en->") && /->(ja|zh-Hans|ar)$/.test(x.pair),
    )) {
      if (o.kind !== "same") expect([o.id, o.verdict]).toEqual([o.id, "script"]);
    }
    const text = report(cases, echoed);
    expect(text).toContain("variant fewShot | model fake");
    for (const o of list.filter((x) => x.target === READ))
      expect(text).toContain(`[${o.id}] ${o.kind}`);
  });

  it.skipIf(OUT === undefined)("reports the model outputs in PERMARK_EVAL_OUT", () => {
    const outputs: EvalOutputs = JSON.parse(readFileSync(OUT ?? "", "utf8"));
    console.log(report(evalCases(set.cases), outputs));
  });
});
