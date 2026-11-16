import { describe, expect, it } from "vitest";
import { browserOutcome, passed, summaryMarkdown, type RunFacts } from "./summary.ts";

const REPORT = {
  suites: [
    {
      title: "render.spec.ts",
      specs: [],
      suites: [
        {
          title: "light",
          specs: [
            {
              title: "demo/README.md",
              tests: [
                {
                  status: "expected",
                  annotations: [{ type: "known issue", description: "edit: remote picture (why)" }],
                  results: [],
                },
              ],
            },
            {
              title: "corpus/math.md",
              tests: [
                {
                  status: "unexpected",
                  results: [
                    {
                      status: "failed",
                      errors: [
                        {
                          message:
                            "\u001b[31mError: expect(locator).toHaveScreenshot(expected) failed\u001b[39m\n  diff",
                        },
                      ],
                      attachments: [
                        { name: "light-view-first-diff.png", path: "/r/light-view-first-diff.png" },
                        { name: "trace", path: "/r/trace.zip" },
                      ],
                    },
                  ],
                },
              ],
            },
            {
              title: "corpus/code.md",
              tests: [
                {
                  status: "unexpected",
                  results: [
                    { status: "failed", errors: [{ message: "Error: guards\nExpected: []" }] },
                  ],
                },
              ],
            },
            { title: "corpus/flaky.md", tests: [{ status: "flaky", results: [] }] },
          ],
        },
      ],
    },
    {
      title: "security.spec.ts",
      specs: [],
      suites: [{ title: "security", specs: [{ title: "x", tests: [{ status: "skipped" }] }] }],
    },
  ],
};

describe("browserOutcome", () => {
  it("counts tests and tells pixel-only failures from the rest", () => {
    const outcome = browserOutcome(REPORT);
    expect(outcome).toMatchObject({ passed: 1, failed: 2, flaky: 1, skipped: 1 });
    expect(outcome.failures).toEqual([
      {
        title: "light > corpus/math.md",
        error: "Error: expect(locator).toHaveScreenshot(expected) failed",
        pixelsOnly: true,
        images: ["/r/light-view-first-diff.png"],
      },
      { title: "light > corpus/code.md", error: "Error: guards", pixelsOnly: false, images: [] },
    ]);
    expect(outcome.known).toEqual(["light > demo/README.md: edit: remote picture (why)"]);
  });

  it("reads a broken report as nothing run", () => {
    expect(browserOutcome(null)).toEqual({
      passed: 0,
      failed: 0,
      flaky: 0,
      skipped: 0,
      failures: [],
      known: [],
    });
  });
});

describe("passed + summaryMarkdown", () => {
  const facts = (update: boolean): RunFacts => ({
    stamp: "20261115-230000",
    steps: [{ name: "render corpus", ok: true, seconds: 2 }],
    browser: browserOutcome(REPORT),
    corpus: { pages: 30, files: 35, folders: ["demo", "corpus"] },
    pixels: "on",
    pending: 0,
    pendingDir: "/b/pending",
    report: "/run/report/index.html",
    update,
  });

  it("fails a normal run on any failure, an update run only on what is not a pixel shot", () => {
    expect(passed(facts(false))).toBe(false);
    expect(passed(facts(true))).toBe(false);
    const pixelsOnly = facts(true);
    pixelsOnly.browser = {
      ...browserOutcome(REPORT),
      failures: browserOutcome(REPORT).failures.slice(0, 1),
    };
    expect(passed(pixelsOnly)).toBe(true);
  });

  it("writes the verdict, steps, corpus and failures", () => {
    const md = summaryMarkdown(facts(false));
    expect(md).toContain("# Deep run 20261115-230000: FAIL");
    expect(md).toContain("| render corpus | ok | 2.0 s |");
    expect(md).toContain("30 Markdown pages");
    expect(md).toContain(
      "- light > corpus/math.md (pixels): Error: expect(locator).toHaveScreenshot",
    );
    expect(md).toContain("  - /r/light-view-first-diff.png");
    expect(md).toContain("## Known issues (known-issues.json)");
  });
});
