// `npm run ux:probe -- <scenario.json | "inline steps"> [options]` (spec: probe.md §8). Exit 0 = ran (flags are facts),
// 2 = usage | setup error.

import { existsSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runScenario } from "./run.ts";
import { parseOptions, scenarioFrom, UsageError } from "./options.ts";

const APP = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

const USAGE = `usage: npm run ux:probe -- <scenario.json | "inline steps"> [--engine chromium|msedge|webkit]
  [--theme light|dark] [--width N] [--height N] [--motion reduce|full] [--locale en-US] [--no-axe]
  [--out <dir>] [--app <app dir>] [--url <running mock url>]
inline: "seed project; seed doc README.md; click role=button[name='Review']; press Escape; probe"`;

function stamp(): string {
  return new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15);
}

async function main(): Promise<number> {
  let options;
  let scenario;
  try {
    options = parseOptions(process.argv.slice(2));
    const source = options.source;
    const file = source.endsWith(".json") ? path.resolve(source) : null;
    const raw: unknown =
      file !== null && existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : source;
    if (file !== null && !existsSync(file)) throw new UsageError(`no such scenario file: ${file}`);
    scenario = scenarioFrom(raw, options);
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    console.error(USAGE);
    return 2;
  }
  const out = options.out ?? path.join(tmpdir(), "permark-ux-probe", `${stamp()}-${scenario.name}`);
  console.log(`out: ${out}`);
  try {
    const report = await runScenario({
      scenario,
      out,
      appDir: options.app ?? APP,
      ...(options.url !== undefined ? { url: options.url } : {}),
      axe: options.axe,
      log: (line) => console.log(line),
    });
    if (report.errors.length > 0)
      console.log(`page errors: ${report.errors.length} (report.json errors[])`);
    console.log(`report: ${path.join(out, "report.json")}`);
    return 0;
  } catch (e) {
    console.error(`probe failed: ${e instanceof Error ? (e.stack ?? e.message) : String(e)}`);
    return 2;
  }
}

process.exitCode = await main();
