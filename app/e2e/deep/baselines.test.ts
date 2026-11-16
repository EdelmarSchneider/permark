import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  accept,
  baselineNames,
  collectPending,
  matches,
  metaMismatch,
  pendingNames,
  readMeta,
  type BaselineMeta,
} from "./baselines.ts";

const META: BaselineMeta = { playwright: "1.64.1", browser: "140.0", os: "win32 10.0.26300" };

let home: string | null = null;

function scratch(): string {
  home = mkdtempSync(path.join(tmpdir(), "permark-deep-baselines-"));
  return home;
}

function put(file: string, text: string): void {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, text);
}

afterEach(() => {
  if (home !== null) rmSync(home, { recursive: true, force: true });
  home = null;
});

describe("metaMismatch", () => {
  it("says what changed, nothing for the same machine or no baselines yet", () => {
    expect(metaMismatch(null, META)).toBeNull();
    expect(metaMismatch(META, { ...META })).toBeNull();
    expect(metaMismatch(META, { ...META, browser: "141.0" })).toBe("browser 140.0 -> 141.0");
  });
});

describe("matches", () => {
  it("reads * within a name part, ** across parts, all for everything", () => {
    expect(matches("all", "demo__README/light-view-first")).toBe(true);
    expect(matches("demo__README/*", "demo__README/light-view-first")).toBe(true);
    expect(matches("*/dark-*", "demo__README/dark-view-first")).toBe(true);
    expect(matches("*", "demo__README/light-view-first")).toBe(false);
    expect(matches("**first", "corpus__code/light-view-first")).toBe(true);
    expect(matches("corpus__code/light-view-first", "corpus__code/light-view-first")).toBe(true);
    expect(matches("corpus.code/*", "corpusXcode/a")).toBe(false);
  });
});

describe("collectPending + accept", () => {
  it("moves a run's differing shots to pending, then accepted ones over the baselines with their meta", () => {
    const dir = scratch();
    const baselines = path.join(dir, "baselines");
    const results = path.join(dir, "results");
    put(path.join(baselines, "p", "light-view-first.png"), "old");
    put(path.join(baselines, "p", "light-view-code-1.png"), "same");
    const before = baselineNames(baselines);
    // a differing shot: Playwright leaves actual + diff in the test's folder
    put(path.join(results, "render-p-light", "p", "light-view-first-actual.png"), "new");
    put(path.join(results, "render-p-light", "p", "light-view-first-diff.png"), "diff");
    put(path.join(results, "render-p-light", "p", "light-view-first-expected.png"), "old");
    put(path.join(results, "render-q-dark", "trace.zip"), "");
    // a shot with no baseline: written beside the baselines by the run
    put(path.join(baselines, "q", "dark-view-first.png"), "fresh");

    expect(collectPending(results, baselines, before)).toEqual([
      "p/light-view-first",
      "q/dark-view-first",
    ]);
    expect(existsSync(path.join(baselines, "q", "dark-view-first.png"))).toBe(false);
    expect(readFileSync(path.join(baselines, "p", "light-view-first.png"), "utf8")).toBe("old");
    expect(pendingNames(baselines)).toEqual(["p/light-view-first", "q/dark-view-first"]);
    const review = readFileSync(path.join(baselines, "pending", "index.html"), "utf8");
    expect(review).toContain("p/light-view-first");
    expect(review).toContain("<em>new</em>");

    expect(accept(baselines, "p/*", META)).toEqual(["p/light-view-first"]);
    expect(readFileSync(path.join(baselines, "p", "light-view-first.png"), "utf8")).toBe("new");
    expect(readMeta(baselines)).toEqual(META);
    expect(pendingNames(baselines)).toEqual(["q/dark-view-first"]);

    expect(accept(baselines, "all", META)).toEqual(["q/dark-view-first"]);
    expect(existsSync(path.join(baselines, "pending"))).toBe(false);
    expect(readFileSync(path.join(baselines, "q", "dark-view-first.png"), "utf8")).toBe("fresh");
  });

  it("accepts nothing and keeps the meta when nothing matches", () => {
    const dir = scratch();
    put(path.join(dir, "pending", "p", "a.png"), "x");
    expect(accept(dir, "q/*", META)).toEqual([]);
    expect(readMeta(dir)).toBeNull();
    expect(pendingNames(dir)).toEqual(["p/a"]);
  });
});
