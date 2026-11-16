// `summary.md` of a deep run, from the steps' outcomes and Playwright's JSON report.

/** One step of the run (render the corpus, sanitizer properties, server, browser specs). */
export type Step = { name: string; ok: boolean; seconds: number; note?: string };

/** A browser test that did not pass. */
export type Failure = { title: string; error: string; pixelsOnly: boolean; images: string[] };

export type BrowserOutcome = {
  passed: number;
  failed: number;
  flaky: number;
  skipped: number;
  failures: Failure[];
  /** Findings listed in `known-issues.json` that tests saw (annotations `known issue`). */
  known: string[];
};

type Json = Record<string, unknown>;

const isJson = (v: unknown): v is Json => typeof v === "object" && v !== null && !Array.isArray(v);
const list = (v: unknown): Json[] => (Array.isArray(v) ? v.filter(isJson) : []);
const text = (v: unknown): string => (typeof v === "string" ? v : "");

/** A pixel shot that differs or has no baseline yet (an update run turns these into candidates). */
function isPixelError(message: string): boolean {
  return message.includes("toHaveScreenshot") || message.includes("A snapshot doesn't exist");
}

/** Strips terminal colors (the report keeps them in error messages). */
function plain(message: string): string {
  return message.replace(/\u001b\[[0-9;]*m/g, "");
}

/** The outcome of the browser specs from Playwright's JSON report. */
export function browserOutcome(report: unknown): BrowserOutcome {
  const outcome: BrowserOutcome = {
    passed: 0,
    failed: 0,
    flaky: 0,
    skipped: 0,
    failures: [],
    known: [],
  };
  const visit = (suite: Json, titles: string[]): void => {
    const here = text(suite["title"]);
    const path = here === "" || here.endsWith(".spec.ts") ? titles : [...titles, here];
    for (const spec of list(suite["specs"])) {
      for (const t of list(spec["tests"])) {
        for (const note of list(t["annotations"])) {
          if (text(note["type"]) === "known issue") {
            outcome.known.push(
              `${[...path, text(spec["title"])].join(" > ")}: ${text(note["description"])}`,
            );
          }
        }
        const status = text(t["status"]);
        if (status === "expected") outcome.passed++;
        else if (status === "flaky") outcome.flaky++;
        else if (status === "skipped") outcome.skipped++;
        else {
          outcome.failed++;
          const results = list(t["results"]);
          const last = results.at(-1) ?? {};
          const errors = list(last["errors"]).map((e) => plain(text(e["message"])));
          const images = list(last["attachments"])
            .filter((a) => text(a["name"]).endsWith("-diff.png"))
            .map((a) => text(a["path"]))
            .filter((p) => p !== "");
          outcome.failures.push({
            title: [...path, text(spec["title"])].join(" > "),
            error: (errors[0] ?? text(last["status"])).split("\n")[0] ?? "",
            pixelsOnly: errors.length > 0 && errors.every(isPixelError),
            images,
          });
        }
      }
    }
    for (const child of list(suite["suites"])) visit(child, path);
  };
  if (isJson(report)) for (const suite of list(report["suites"])) visit(suite, []);
  return outcome;
}

export type RunFacts = {
  stamp: string;
  steps: Step[];
  browser: BrowserOutcome | null;
  corpus: { pages: number; files: number; folders: string[] };
  pixels: string;
  pending: number;
  pendingDir: string;
  report: string;
  update: boolean;
};

/** Passed: every step ok and no browser failure (an update run only fails on what is not a pixel shot). */
export function passed(facts: RunFacts): boolean {
  if (!facts.steps.every((s) => s.ok)) return false;
  const failures = facts.browser?.failures ?? [];
  return facts.update ? failures.every((f) => f.pixelsOnly) : failures.length === 0;
}

export function summaryMarkdown(facts: RunFacts): string {
  const ok = passed(facts);
  const total = facts.steps.reduce((sum, s) => sum + s.seconds, 0);
  const lines = [
    `# Deep run ${facts.stamp}: ${ok ? "PASS" : "FAIL"}${facts.update ? " (update)" : ""}`,
    "",
    "| step | result | time |",
    "|---|---|---:|",
    ...facts.steps.map(
      (s) =>
        `| ${s.name} | ${s.ok ? "ok" : "FAILED"}${s.note ? ` (${s.note})` : ""} | ${s.seconds.toFixed(1)} s |`,
    ),
    `| total | | ${total.toFixed(1)} s |`,
    "",
    `- corpus: ${facts.corpus.pages} Markdown pages, ${facts.corpus.files} rendered files (${facts.corpus.folders.join(", ")})`,
    `- pixels: ${facts.pixels}`,
  ];
  if (facts.pending > 0) {
    lines.push(
      `- pending pixel candidates: ${facts.pending} (review: ${facts.pendingDir}/index.html)`,
    );
  }
  const b = facts.browser;
  if (b !== null) {
    lines.push(
      `- browser tests: ${b.passed} passed, ${b.failed} failed, ${b.flaky} flaky, ${b.skipped} skipped`,
      `- report: ${facts.report}`,
    );
    if (b.known.length > 0) {
      lines.push("", "## Known issues (known-issues.json)", "", ...b.known.map((k) => `- ${k}`));
    }
    if (b.failures.length > 0) {
      lines.push("", "## Failures", "");
      for (const f of b.failures) {
        lines.push(`- ${f.title}${f.pixelsOnly ? " (pixels)" : ""}: ${f.error}`);
        for (const image of f.images) lines.push(`  - ${image}`);
      }
    }
  }
  return `${lines.join("\n")}\n`;
}
