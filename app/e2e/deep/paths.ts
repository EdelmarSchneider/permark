// Where the deep suite reads and writes (every place outside the repository but the text goldens). Each default can be
// moved by its environment variable; the runner passes the resolved values on to the server and the specs.

import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** The app folder (`app/`). */
export const APP_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/** The repository root. */
export const REPO_DIR = path.dirname(APP_DIR);

/** The deep suite's folder (`app/e2e/deep`). */
export const DEEP_DIR = path.join(APP_DIR, "e2e", "deep");

/** Vite mode of the deep server: the mock backend with the rendered corpus in place of the mock documents. */
export const DEEP_MODE = "deep";

/** The browser engine the pixel baselines are recorded with. */
export const ENGINE = "chromium";

function fromEnv(name: string): string | null {
  const value = process.env[name];
  return value !== undefined && value !== "" ? path.resolve(value) : null;
}

/** Runs, the rendered corpus and the server's cache: `<temp>/permark-deep`. */
export function deepHome(): string {
  return fromEnv("PERMARK_DEEP_HOME") ?? path.join(tmpdir(), "permark-deep");
}

/** The rendered corpus (`render_corpus` writes it): views, sources, sheets and `project/` with every file. */
export function corpusOut(): string {
  return fromEnv("PERMARK_DEEP_OUT") ?? path.join(deepHome(), "corpus");
}

/** Pixel baselines of this OS + engine; default: `deep-baselines/<os>-<engine>` beside the repository checkout. */
export function baselinesDir(): string {
  return (
    fromEnv("PERMARK_DEEP_BASELINES") ??
    path.join(path.dirname(REPO_DIR), "deep-baselines", `${process.platform}-${ENGINE}`)
  );
}

/** Text goldens (in the repository: reviewed as diffs). */
export const GOLDENS_DIR = path.join(DEEP_DIR, "goldens");

/** Files the deep server serves from the rendered corpus's project (`mockAssetUrl` in deep mode). */
export const FILES_ROUTE = "/@permark-deep/project/";
