import { describe, expect, it } from "vitest";
import type { FolderJob, FolderSummary } from "../../api";
import { fill, type MessageKey, type PluralKey } from "../../core/i18n";
import en from "../../core/i18n/locales/en";
import {
  badgeText,
  failedKey,
  folderName,
  isWorking,
  parentOf,
  shortNumber,
  skippedReasons,
  summaryText,
  timeUnit,
  typeName,
  type Say,
} from "./folders";
import { infoOf } from "./tokenizers";

const say: Say = {
  t: (key: MessageKey, params) => fill(en[key], params),
  tn: (base: PluralKey, n, params) =>
    fill(n === 1 ? en[`${base}.one`] : en[`${base}.other`], { n, ...params }),
  number: (n) => new Intl.NumberFormat("en").format(n),
};

describe("the badge of a counted folder", () => {
  it("is the whole number below 10 000, then 12.3k, then 1.2M", () => {
    const table: [number, string][] = [
      [0, "0"],
      [842, "842"],
      [9_999, "9,999"],
      [10_000, "10k"],
      [12_318, "12.3k"],
      [99_949, "99.9k"],
      [999_949, "999.9k"],
      [999_950, "1M"],
      [1_000_000, "1M"],
      [1_234_567, "1.2M"],
      [25_000_000, "25M"],
    ];
    for (const [n, shown] of table) expect(shortNumber(n, "en"), String(n)).toBe(shown);
  });

  it("marks an estimate with ≈ before the number and a stale count with * after it", () => {
    expect(badgeText(842, false, false, "en")).toBe("842");
    expect(badgeText(842, true, false, "en")).toBe("≈842");
    expect(badgeText(12_318, false, true, "en")).toBe("12.3k*");
    expect(badgeText(12_318, true, true, "en")).toBe("≈12.3k*");
  });

  it("follows the language of the window", () => {
    expect(shortNumber(12_318, "de")).toBe("12,3k");
    expect(shortNumber(9_999, "de")).toBe("9.999");
  });
});

describe("time, reasons, failures", () => {
  it("says seconds up to a minute and a half, then whole minutes rounded up", () => {
    expect(timeUnit(1)).toEqual({ unit: "seconds", n: 1 });
    expect(timeUnit(0)).toEqual({ unit: "seconds", n: 1 });
    expect(timeUnit(89)).toEqual({ unit: "seconds", n: 89 });
    expect(timeUnit(90)).toEqual({ unit: "minutes", n: 2 });
    expect(timeUnit(121)).toEqual({ unit: "minutes", n: 3 });
  });

  it("has words for every failure and tells which jobs are working", () => {
    for (const reason of ["notFound", "unavailable", "io"] as const) {
      expect(en[failedKey(reason)].length).toBeGreaterThan(5);
    }
    const jobs: [FolderJob, boolean][] = [
      [{ state: "idle" }, false],
      [{ state: "scanning", folder: "" }, true],
      [{ state: "confirm", folder: "", files: 1, bytes: 1, seconds: 1 }, false],
      [{ state: "refused", folder: "", files: 1, bytes: 1, reason: "files" }, false],
      [
        { state: "counting", folder: "", done: 1, total: 2, tokenizer: "o200k", approx: false },
        true,
      ],
      [{ state: "done", folder: "" }, false],
      [{ state: "cancelled", folder: "" }, false],
      [{ state: "failed", folder: "", reason: "io" }, false],
    ];
    for (const [job, working] of jobs) expect(isWorking(job), job.state).toBe(working);
  });

  it("finds the parent folder of a path", () => {
    expect(parentOf("docs/a.md")).toBe("docs");
    expect(parentOf("docs/deep/a.md")).toBe("docs/deep");
    expect(parentOf("a.md")).toBe("");
    expect(parentOf("")).toBe("");
  });

  it("names a folder, and the types of a summary", () => {
    expect(folderName("", say)).toBe("the project");
    expect(folderName("docs/deep", say)).toBe("docs/deep");
    expect(typeName("md", say)).toBe(".md");
    expect(typeName("", say)).toBe("(no extension)");
    expect(typeName("*", say)).toBe("other types");
  });
});

const summary: FolderSummary = {
  folder: "docs",
  tokenizer: "o200k",
  approx: false,
  tokens: 12_318,
  counted: 214,
  skipped: 9,
  skippedBinary: 7,
  skippedLarge: 2,
  skippedUnreadable: 0,
  skippedList: [],
  byType: [
    { ext: "md", files: 120, tokens: 9_800 },
    { ext: "json", files: 30, tokens: 1_900 },
    { ext: "", files: 4, tokens: 400 },
    { ext: "*", files: 60, tokens: 218 },
  ],
  top: [
    { path: "docs/guide.md", tokens: 2_276 },
    { path: "docs/spec.md", tokens: 1_900 },
  ],
  millis: 420,
  cached: 100,
};

describe("the summary as text", () => {
  it("says the total with its tokenizer, the files, the types and the biggest files", () => {
    const text = summaryText(summary, infoOf("o200k"), say).split("\n");
    expect(text).toEqual([
      "docs: 12,318 tokens · GPT-4o (Exact)",
      "214 files counted, 9 files skipped (7 not text, 2 over 5 MB)",
      "By file type: .md 9,800 (120 files) · .json 1,900 (30 files) · (no extension) 400 (4 files) · other types 218 (60 files)",
      "Largest files: docs/guide.md 2,276 · docs/spec.md 1,900",
    ]);
  });

  it("marks an estimate and names the project folder; no skipped files, no reasons", () => {
    const text = summaryText(
      { ...summary, folder: "", approx: true, tokenizer: "claude", skipped: 0, tokens: 1 },
      infoOf("claude"),
      say,
    ).split("\n");
    expect(text[0]).toBe("the project: 1 token · Claude (Approximate)");
    expect(text[1]).toBe("214 files counted");
    expect(skippedReasons({ ...summary, skippedBinary: 0, skippedLarge: 0 }, say)).toBe("");
  });
});
