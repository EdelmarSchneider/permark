// The settings of one deep run, as the runner (`cli.ts`) hands them to the Playwright process through the environment.

import { readFileSync } from "node:fs";
import path from "node:path";
import type { RenderedView } from "./digest.ts";
import { baselinesDir, corpusOut, deepHome } from "./paths.ts";

/** The deep server the runner started (`PERMARK_DEEP_URL`). */
export function serverUrl(): string {
  const url = process.env["PERMARK_DEEP_URL"];
  if (url === undefined || url === "") {
    throw new Error("no deep server: run the suite through `npm run deep`");
  }
  return url;
}

/** This run's folder (results, report, summary). */
export function runDir(): string {
  return process.env["PERMARK_DEEP_RUN"] ?? path.join(deepHome(), "adhoc");
}

/** Text goldens are written instead of compared (`--update`). */
export const UPDATE = process.env["PERMARK_DEEP_UPDATE"] === "1";

/** Light theme, View only (`--quick`). */
export const QUICK = process.env["PERMARK_DEEP_QUICK"] === "1";

/** Pixel shots are compared (off when the baselines were drawn by another browser | OS build). */
export const PIXELS = process.env["PERMARK_DEEP_PIXELS"] !== "off";

export { baselinesDir };

/** The rendered Markdown pages of the corpus (`render_corpus`), in the renderer's order. */
export function corpusPages(): RenderedView[] {
  const file = path.join(corpusOut(), "documents.json");
  const all: unknown = JSON.parse(readFileSync(file, "utf8"));
  if (typeof all !== "object" || all === null) return [];
  return Object.values(all).filter(
    (view): view is RenderedView =>
      typeof view === "object" &&
      view !== null &&
      Reflect.get(view, "format") === "markdown" &&
      typeof Reflect.get(view, "path") === "string",
  );
}
