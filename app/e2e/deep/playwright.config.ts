// Playwright for the deep suite only (`npm run deep` starts the server and passes the run's settings). Specs are
// `*.spec.ts`: vitest runs `*.test.ts` and never picks them up.

import { defineConfig } from "@playwright/test";
import path from "node:path";
import { baselinesDir, runDir, UPDATE } from "./settings.ts";

const run = runDir();
const workers = Number(process.env["PERMARK_DEEP_WORKERS"] ?? "") || 4;

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.ts",
  outputDir: path.join(run, "results"),
  fullyParallel: true,
  workers,
  retries: 0,
  timeout: 240_000,
  reporter: [
    ["list"],
    ["html", { outputFolder: path.join(run, "report"), open: "never" }],
    ["json", { outputFile: path.join(run, "results.json") }],
  ],
  // a shot never overwrites a baseline; an update run writes missing ones, which the runner moves to `pending/` with
  // the differing ones for review
  updateSnapshots: UPDATE ? "missing" : "none",
  expect: {
    timeout: 15_000,
    toHaveScreenshot: {
      pathTemplate: path.join(baselinesDir(), "{arg}{ext}"),
      maxDiffPixelRatio: 0.002,
      animations: "disabled",
      caret: "hide",
      scale: "css",
    },
  },
  use: {
    browserName: "chromium",
    headless: true,
    viewport: { width: 1200, height: 800 },
    deviceScaleFactor: 1,
    locale: "en-US",
    timezoneId: "UTC",
    reducedMotion: "reduce",
    trace: "off",
  },
});
