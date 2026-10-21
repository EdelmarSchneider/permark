// `npm run ux:calibrate -- [--at all|now|before|after] [--only <id>] [--engine ...] [--out <dir>]` (spec: probe.md §9).
// Runs each fixture's scenario at the commits asked (default all: before + after + now; a temporary git worktree for
// before | after) and checks the expected flags. Exit 1 when any expectation is not met, 2 on usage | setup errors.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { unmet } from "../probe/facts.ts";
import { oneOf, type Engine } from "../probe/scenario.ts";
import { commitAt, parseFixture, type At, type Fixture } from "./fixture.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const APP = path.resolve(HERE, "..", "..");

function git(args: string[], cwd = APP): string {
  const r = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr.trim()}`);
  return r.stdout.trim();
}

/** A worktree of the repository at `commit` with its own `npm ci` (a borrowed node_modules would be outside its root:
 *  Vite refuses to serve the fonts from there); `remove` undoes everything it made. */
function worktreeAt(commit: string, log: (s: string) => void): { appDir: string; remove(): void } {
  const sha = git(["rev-parse", "--verify", `${commit}^{commit}`]);
  const dir = path.join(tmpdir(), "permark-ux-calibrate", sha.slice(0, 10));
  const top = git(["rev-parse", "--show-toplevel"]);
  const remove = (): void => {
    spawnSync("git", ["worktree", "remove", "--force", dir], { cwd: top });
    rmSync(dir, { recursive: true, force: true });
    spawnSync("git", ["worktree", "prune"], { cwd: top });
  };
  if (existsSync(dir)) remove();
  git(["worktree", "add", "--detach", dir, sha], top);
  const appDir = path.join(dir, "app");
  try {
    log(`  ${sha.slice(0, 10)}: npm ci`);
    // no install scripts: the mock app needs none (and npm may refuse the old lockfile's ones)
    const npm = spawnSync("npm ci --ignore-scripts --prefer-offline --no-audit --no-fund", {
      cwd: appDir,
      encoding: "utf8",
      shell: true,
      // the `npm run` around us passes its config down as npm_* variables: a fresh npm reads its own
      env: Object.fromEntries(Object.entries(process.env).filter(([k]) => !/^npm_/i.test(k))),
    });
    if (npm.status !== 0) {
      throw new Error(
        `npm ci at ${sha}: ${npm.stderr.trim().split(/\r?\n/).slice(-6).join(" | ")}`,
      );
    }
    return { appDir, remove };
  } catch (e) {
    remove();
    throw e;
  }
}

function fixtures(only: string | null): Fixture[] {
  return readdirSync(HERE)
    .filter((f) => f.endsWith(".json") && f !== "not-reproducible.json")
    .sort()
    .map((f) => {
      try {
        return parseFixture(JSON.parse(readFileSync(path.join(HERE, f), "utf8")));
      } catch (e) {
        throw new Error(`${f}: ${e instanceof Error ? e.message : String(e)}`);
      }
    })
    .filter((fx) => only === null || fx.id === only);
}

type Args = { at: At[]; only: string | null; engine: Engine | null; out: string };

function parseArgs(argv: string[]): Args {
  const args: Args = {
    at: ["before", "after", "now"],
    only: null,
    engine: null,
    out: path.join(
      tmpdir(),
      "permark-ux-calibrate",
      "runs",
      new Date().toISOString().replace(/[:.]/g, "-"),
    ),
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = argv[i + 1];
    if (v === undefined && a !== undefined) throw new Error(`${a}: value expected`);
    switch (a) {
      case "--at":
        args.at =
          v === "all"
            ? ["before", "after", "now"]
            : [oneOf(v, ["now", "before", "after"], a, "now")];
        i++;
        break;
      case "--only":
        args.only = v ?? null;
        i++;
        break;
      case "--engine":
        args.engine = oneOf(v, ["chromium", "msedge", "webkit"], a, "chromium");
        i++;
        break;
      case "--out":
        args.out = path.resolve(v ?? "");
        i++;
        break;
      default:
        throw new Error(`unknown option ${a}`);
    }
  }
  return args;
}

async function main(): Promise<number> {
  let args: Args;
  let list: Fixture[];
  try {
    args = parseArgs(process.argv.slice(2));
    list = fixtures(args.only);
    if (list.length === 0) throw new Error("no fixture matches");
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    console.error(
      "usage: npm run ux:calibrate -- [--at all|now|before|after] [--only <id>] [--engine chromium|msedge|webkit] [--out <dir>]",
    );
    return 2;
  }
  // the browser half needs the dev dependencies (Playwright, axe): missing -> say how to get them
  let runScenario: typeof import("../probe/run.ts").runScenario;
  try {
    ({ runScenario } = await import("../probe/run.ts"));
  } catch (e) {
    console.error(`probe dependencies missing (${e instanceof Error ? e.message : String(e)})`);
    console.error("run `npm ci` in app/ first");
    return 2;
  }
  mkdirSync(args.out, { recursive: true });
  console.log(`out: ${args.out}`);
  let failed = 0;
  for (const at of args.at) {
    for (const fx of list) {
      const expect = fx.expect[at];
      if (expect === undefined || (at !== "now" && fx.fix === null)) continue;
      const commit = commitAt(fx, at);
      let tree: { appDir: string; remove(): void } | null = null;
      const label = `${fx.id} @${at}${commit ? ` (${commit})` : ""}`;
      try {
        if (commit !== null) tree = worktreeAt(commit, (s) => console.log(s));
        const scenario =
          args.engine === null ? fx.scenario : { ...fx.scenario, engine: args.engine };
        const report = await runScenario({
          scenario,
          out: path.join(args.out, `${fx.id}-${at}`),
          appDir: tree?.appDir ?? APP,
          axe: false,
        });
        const misses = unmet(
          expect,
          report.steps.map((s) => ({ name: s.name, flags: s.flags })),
        );
        const shown = report.steps
          .filter((s) => s.probed)
          .map((s) => `${s.name}=[${[...new Set(s.flags.map((f) => f.id))].join(",")}]`)
          .join(" ");
        if (misses.length === 0) console.log(`${label}: pass  ${shown}`);
        else {
          failed += 1;
          console.log(`${label}: FAIL ${misses.join("; ")}  ${shown}`);
        }
      } catch (e) {
        failed += 1;
        console.log(`${label}: FAIL ${e instanceof Error ? e.message : String(e)}`);
      } finally {
        tree?.remove();
      }
    }
  }
  return failed === 0 ? 0 : 1;
}

process.exitCode = await main();
