// Pixel baselines outside the repository: `<baselines>/<page>/<shot>.png` + `meta.json` (what recorded them). An update
// run leaves candidates in `<baselines>/pending/` with a review page; accept moves the chosen ones over the baselines.

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

/** What drew a set of baselines: another browser build, Playwright or OS build draws text and edges differently. */
export type BaselineMeta = { playwright: string; browser: string; os: string };

const META = "meta.json";
const PENDING = "pending";
const ACTUAL = "-actual.png";
const DIFF = "-diff.png";

export function readMeta(dir: string): BaselineMeta | null {
  const file = path.join(dir, META);
  if (!existsSync(file)) return null;
  const value: unknown = JSON.parse(readFileSync(file, "utf8"));
  if (typeof value !== "object" || value === null) return null;
  const field = (key: string): string => {
    const v: unknown = Reflect.get(value, key);
    return typeof v === "string" ? v : "";
  };
  return { playwright: field("playwright"), browser: field("browser"), os: field("os") };
}

/** Why the baselines in `dir` cannot be compared with this machine's shots (`null`: they can). */
export function metaMismatch(recorded: BaselineMeta | null, now: BaselineMeta): string | null {
  if (recorded === null) return null;
  const differ = (["playwright", "browser", "os"] as const).filter((k) => recorded[k] !== now[k]);
  if (differ.length === 0) return null;
  return differ.map((k) => `${k} ${recorded[k]} -> ${now[k]}`).join(", ");
}

function filesUnder(dir: string, rel = ""): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const sub = rel === "" ? entry.name : `${rel}/${entry.name}`;
    return entry.isDirectory() ? filesUnder(path.join(dir, entry.name), sub) : [sub];
  });
}

/** The baseline shots in `dir` (`<page>/<shot>`), candidates aside. */
export function baselineNames(dir: string): Set<string> {
  return new Set(
    filesUnder(dir)
      .filter((rel) => rel.endsWith(".png") && !rel.startsWith(`${PENDING}/`))
      .map((rel) => rel.slice(0, -".png".length)),
  );
}

/**
 * Turns an update run's shots into candidates in `<baselines>/pending/`: shots with no baseline before the run
 * (`before`; the run wrote them beside the baselines) move there, and shots that differ from their baseline come from
 * `results` (one folder per test with `<shot>-actual.png` + `-diff.png`; a missing shot may leave its actual there
 * too). Returns the candidates (`<page>/<shot>`).
 */
export function collectPending(results: string, baselines: string, before: Set<string>): string[] {
  const pending = path.join(baselines, PENDING);
  const names = new Set<string>();
  for (const name of baselineNames(baselines)) {
    if (before.has(name)) continue;
    const target = path.join(pending, `${name}.png`);
    mkdirSync(path.dirname(target), { recursive: true });
    renameSync(path.join(baselines, `${name}.png`), target);
    names.add(name);
  }
  if (existsSync(results)) {
    for (const testDir of readdirSync(results, { withFileTypes: true })) {
      if (!testDir.isDirectory()) continue;
      const root = path.join(results, testDir.name);
      for (const rel of filesUnder(root)) {
        if (!rel.endsWith(ACTUAL)) continue;
        const name = rel.slice(0, -ACTUAL.length);
        const target = path.join(pending, `${name}.png`);
        mkdirSync(path.dirname(target), { recursive: true });
        copyFileSync(path.join(root, rel), target);
        const diff = path.join(root, `${name}${DIFF}`);
        if (existsSync(diff)) copyFileSync(diff, path.join(pending, `${name}.diff.png`));
        names.add(name);
      }
    }
  }
  writeReview(baselines);
  return [...names].sort();
}

/** `*` = any characters but `/`, `**` = any characters; `all` = every name. */
export function matches(pattern: string, name: string): boolean {
  if (pattern === "all") return true;
  const source = pattern
    .split("**")
    .map((part) =>
      part
        .split("*")
        .map((p) => p.replace(/[.+?^${}()|[\]\\]/g, "\\$&"))
        .join("[^/]*"),
    )
    .join(".*");
  return new RegExp(`^${source}$`).test(name);
}

/** The candidates waiting in `<baselines>/pending/` (`<page>/<shot>`). */
export function pendingNames(baselines: string): string[] {
  return filesUnder(path.join(baselines, PENDING))
    .filter((rel) => rel.endsWith(".png") && !rel.endsWith(".diff.png"))
    .map((rel) => rel.slice(0, -".png".length))
    .sort();
}

/** Promotes the candidates matching `pattern` to baselines and records `meta`; returns the names accepted. */
export function accept(baselines: string, pattern: string, meta: BaselineMeta): string[] {
  const pending = path.join(baselines, PENDING);
  const taken = pendingNames(baselines).filter((name) => matches(pattern, name));
  for (const name of taken) {
    const target = path.join(baselines, `${name}.png`);
    mkdirSync(path.dirname(target), { recursive: true });
    rmSync(target, { force: true });
    renameSync(path.join(pending, `${name}.png`), target);
    rmSync(path.join(pending, `${name}.diff.png`), { force: true });
  }
  if (taken.length > 0) {
    writeFileSync(path.join(baselines, META), `${JSON.stringify(meta, null, 2)}\n`);
  }
  if (pendingNames(baselines).length === 0) rmSync(pending, { recursive: true, force: true });
  else writeReview(baselines);
  return taken;
}

const escape = (text: string): string =>
  text.replace(
    /[&<>"]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c,
  );

/** `<baselines>/pending/index.html`: each candidate beside its baseline and the difference. */
export function writeReview(baselines: string): void {
  const names = pendingNames(baselines);
  if (names.length === 0) return;
  const rows = names.map((name) => {
    const url = (file: string): string => encodeURI(file).replace(/#/g, "%23");
    const base = existsSync(path.join(baselines, `${name}.png`))
      ? `<img src="${url(`../${name}.png`)}" alt="">`
      : "<em>new</em>";
    const diff = existsSync(path.join(baselines, PENDING, `${name}.diff.png`))
      ? `<img src="${url(`${name}.diff.png`)}" alt="">`
      : "";
    return `<tr><th colspan="3">${escape(name)}</th></tr><tr><td>${base}</td><td><img src="${url(`${name}.png`)}" alt=""></td><td>${diff}</td></tr>`;
  });
  const page = `<!doctype html><meta charset="utf-8"><title>Pending baselines (${names.length})</title>
<style>body{font:14px system-ui;margin:16px}table{border-collapse:collapse}th{text-align:left;padding:12px 4px 4px}td{vertical-align:top;padding:4px;border:1px solid #ccc}img{max-width:560px}</style>
<h1>Pending baselines: ${names.length}</h1>
<p>Baseline | candidate | difference. Accept: <code>npm run deep -- --accept &lt;glob|all&gt;</code> (names as below).</p>
<table><tr><th>baseline</th><th>candidate</th><th>difference</th></tr>${rows.join("")}</table>`;
  writeFileSync(path.join(baselines, PENDING, "index.html"), page);
}
