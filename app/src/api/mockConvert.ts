// What the TOON format and the converter answer in the mock backend: stand-ins with the real shapes. The real
// conversions, counts and judgments are the backend's (tested there); the mock only feeds the dialog.

import type { Analysis } from "./generated/Analysis";
import type { ConvertAnswer } from "./generated/ConvertAnswer";
import { MockPluginError } from "./mockOrganizing";
import type { ConvertSource, PluginMethods } from "./types";

type ConvertMethods = Pick<
  PluginMethods,
  | "fmt-toon.toJson"
  | "convert.analyze"
  | "convert.result"
  | "convert.save"
  | "convert.cancel"
  | "convert.close"
>;

export type ConvertProbe = {
  /** The answer the next analysis gives instead of the stand-in. */
  next: ConvertAnswer | null;
  /** Calls made, `method job`. */
  calls: string[];
  /** Files saved, path -> text. */
  saved: Map<string, string>;
};

const estimate = (text: string): number => Math.max(1, Math.round(text.length / 4));

function standIn(text: string): Analysis {
  const source = estimate(text);
  const view = (id: string, ext: string, share: number, fit: string, reason: string) => ({
    id,
    ext,
    tokens: fit === "no" ? null : Math.round(source * share),
    fit,
    reason,
    best: false,
    preview: fit === "no" ? "" : `${id} of ${text.slice(0, 40)}\n`,
    notes: [],
  });
  const targets = [
    { ...view("toon", "toon", 0.55, "good", "tabular"), best: true },
    view("json", "json", 0.8, "good", "compact"),
    view("jsonl", "jsonl", 0.82, "good", "lines"),
    view("csv", "csv", 0.5, "no", "notTable"),
    view("tsv", "tsv", 0.5, "no", "notTable"),
    view("yaml", "yaml", 0.9, "good", "readable"),
    view("jsonPretty", "json", 1.4, "poor", "readable"),
  ];
  return {
    from: "json",
    bytes: text.length,
    sourceTokens: source,
    tokenizer: null,
    approx: true,
    notes: [],
    targets,
  };
}

export function createMockConvert(textOf: (path: string) => string | null): {
  handlers: {
    [M in keyof ConvertMethods]: (args: ConvertMethods[M]["args"]) => ConvertMethods[M]["result"];
  };
  probe: ConvertProbe;
} {
  const probe: ConvertProbe = { next: null, calls: [], saved: new Map() };
  const jobs = new Map<number, { source: ConvertSource; analysis: Analysis }>();
  const textOfSource = (source: ConvertSource): string =>
    source.kind === "text" ? source.text : (textOf(source.path) ?? "");
  return {
    probe,
    handlers: {
      "fmt-toon.toJson": ({ text }) => JSON.stringify({ toon: text.split("\n").length }, null, 2),
      "convert.analyze": ({ job, source }) => {
        probe.calls.push(`analyze ${job}`);
        const answer = probe.next ?? { status: "ready", analysis: standIn(textOfSource(source)) };
        probe.next = null;
        if (answer.status === "ready") jobs.set(job, { source, analysis: answer.analysis });
        return answer;
      },
      "convert.result": ({ job, target }) => {
        probe.calls.push(`result ${job} ${target}`);
        const found = jobs.get(job);
        if (found === undefined) throw new MockPluginError("notFound", "job");
        return `${target} of ${textOfSource(found.source)}`;
      },
      "convert.save": ({ job, target, beside }) => {
        probe.calls.push(`save ${job} ${target}`);
        const found = jobs.get(job);
        if (found === undefined) throw new MockPluginError("notFound", "job");
        const next = found.source.kind === "file" ? found.source.path : (beside ?? "");
        const view = found.analysis.targets.find((t) => t.id === target);
        const stem = next.replace(/\.[^./]*$/, "");
        let path = `${stem}.${view?.ext ?? target}`;
        for (let n = 2; probe.saved.has(path) || textOf(path) !== null; n++) {
          path = `${stem} (${n}).${view?.ext ?? target}`;
        }
        probe.saved.set(path, `${target} of ${textOfSource(found.source)}`);
        return path;
      },
      "convert.cancel": ({ job }) => {
        probe.calls.push(`cancel ${job}`);
        return null;
      },
      "convert.close": ({ job }) => {
        probe.calls.push(`close ${job}`);
        jobs.delete(job);
        return null;
      },
    },
  };
}
