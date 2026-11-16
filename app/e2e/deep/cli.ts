// `npm run deep -- [options]` (see options.ts USAGE): the deep regression suite, run by hand every few days. Renders the
// corpus with the real renderer, checks the sanitizer's properties, serves the corpus in deep mode and runs the browser
// specs; writes `<temp>/permark-deep/<stamp>/summary.md` + an HTML report, keeps the last three runs. Exit 1 = failed.

import { chromium } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { release } from "node:os";
import path from "node:path";
import { startMock, type MockServer } from "../probe/server.ts";
import {
  accept,
  baselineNames,
  collectPending,
  metaMismatch,
  pendingNames,
  readMeta,
  type BaselineMeta,
} from "./baselines.ts";
import { parseOptions, USAGE, UsageError, type Options } from "./options.ts";
import { APP_DIR, baselinesDir, corpusOut, DEEP_DIR, DEEP_MODE, deepHome } from "./paths.ts";
import {
  browserOutcome,
  passed,
  summaryMarkdown,
  type BrowserOutcome,
  type Step,
} from "./summary.ts";

const KEEP_RUNS = 3;
const RUN_NAME = /^\d{8}-\d{6}$/;
const SRC_TAURI = path.join(APP_DIR, "src-tauri");

function stamp(): string {
  return new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15);
}

/** Runs `fn` as a named step, timed; a thrown error fails the step (its message becomes the note). */
async function step(
  steps: Step[],
  name: string,
  fn: () => Promise<string | void> | string | void,
): Promise<boolean> {
  const start = performance.now();
  console.log(`\n== ${name}`);
  let ok = true;
  let note: string | undefined;
  try {
    note = (await fn()) ?? undefined;
  } catch (e) {
    ok = false;
    note = e instanceof Error ? e.message.split("\n")[0] : String(e);
    console.error(e instanceof Error ? e.message : String(e));
  }
  const seconds = (performance.now() - start) / 1000;
  steps.push({ name, ok, seconds, ...(note !== undefined ? { note } : {}) });
  console.log(`== ${name}: ${ok ? "ok" : "FAILED"} (${seconds.toFixed(1)} s)`);
  return ok;
}

function run(command: string, args: string[], cwd: string, env: NodeJS.ProcessEnv): void {
  const result = spawnSync(command, args, { cwd, env, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`${command} ${args.slice(0, 4).join(" ")} exited ${result.status}`);
}

/** What draws the pixels here: Playwright, the browser build, the OS build (the browser is started once to ask). */
async function machine(): Promise<BaselineMeta> {
  const pkg: unknown = JSON.parse(
    readFileSync(path.join(APP_DIR, "node_modules", "@playwright", "test", "package.json"), "utf8"),
  );
  const version = typeof pkg === "object" && pkg !== null ? Reflect.get(pkg, "version") : null;
  let browser;
  try {
    browser = await chromium.launch();
  } catch (e) {
    throw new Error(
      `no Chromium for Playwright (${e instanceof Error ? e.message.split("\n")[0] : String(e)}): run \`npx playwright install chromium\``,
    );
  }
  try {
    return {
      playwright: typeof version === "string" ? version : "?",
      browser: browser.version(),
      os: `${process.platform} ${release()}`,
    };
  } finally {
    await browser.close();
  }
}

/** Loads the app once so the server has bundled its dependencies before parallel tests ask for them. */
async function warmUp(url: string): Promise<void> {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(url);
    await page.locator("#app > *").first().waitFor({ state: "attached", timeout: 120_000 });
  } finally {
    await browser.close();
  }
}

/** Removes all but the newest runs. */
function prune(home: string): void {
  const runs = existsSync(home)
    ? readdirSync(home)
        .filter((name) => RUN_NAME.test(name))
        .sort()
    : [];
  for (const old of runs.slice(0, Math.max(0, runs.length - KEEP_RUNS))) {
    rmSync(path.join(home, old), { recursive: true, force: true });
  }
}

function corpusFacts(): { pages: number; files: number; folders: string[] } {
  const file = path.join(corpusOut(), "documents.json");
  if (!existsSync(file)) return { pages: 0, files: 0, folders: [] };
  const all: unknown = JSON.parse(readFileSync(file, "utf8"));
  const views = typeof all === "object" && all !== null ? Object.entries(all) : [];
  const pages = views.filter(
    ([, v]) => typeof v === "object" && v !== null && Reflect.get(v, "format") === "markdown",
  ).length;
  const folders = [...new Set(views.map(([key]) => key.split("/")[0] ?? key))];
  return { pages, files: views.length, folders };
}

async function acceptOnly(pattern: string): Promise<number> {
  const baselines = baselinesDir();
  const meta = await machine();
  const taken = accept(baselines, pattern, meta);
  console.log(`accepted ${taken.length} shot(s) into ${baselines}`);
  const left = pendingNames(baselines).length;
  if (left > 0)
    console.log(`still pending: ${left} (${path.join(baselines, "pending", "index.html")})`);
  return taken.length > 0 || pattern !== "all" ? 0 : 1;
}

async function deepRun(options: Options): Promise<number> {
  const home = deepHome();
  const name = stamp();
  const dir = path.join(home, name);
  mkdirSync(dir, { recursive: true });
  prune(home);
  const baselines = baselinesDir();
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    PERMARK_DEEP_OUT: corpusOut(),
    PERMARK_DEEP_RUN: dir,
    PERMARK_DEEP_BASELINES: baselines,
  };
  const steps: Step[] = [];
  let browser: BrowserOutcome | null = null;
  let pixels = "on";
  // set inside a step: a holder, so the checks after it see the assignment
  const held: { server: MockServer | null } = { server: null };
  /** Baselines before an update run: the shots it writes beside them are candidates. */
  let before = new Set<string>();
  console.log(`run: ${dir}\nbaselines: ${baselines}\ncorpus: ${corpusOut()}`);

  try {
    const meta = await machine();
    const recorded = readMeta(baselines);
    const mismatch = metaMismatch(recorded, meta);
    if (options.update) {
      rmSync(path.join(baselines, "pending"), { recursive: true, force: true });
      before = baselineNames(baselines);
    } else if (recorded === null) {
      pixels = "off (no baselines yet: record them with --update, then --accept all)";
    } else if (mismatch !== null) {
      pixels = `off (baselines drawn by another build: ${mismatch}; re-record with --update, then --accept all)`;
    }
    env["PERMARK_DEEP_PIXELS"] = pixels === "on" ? "on" : "off";
    if (options.update) env["PERMARK_DEEP_UPDATE"] = "1";
    if (options.quick) env["PERMARK_DEEP_QUICK"] = "1";

    const rendered = await step(steps, "render corpus (Rust renderer)", () => {
      run(
        "cargo",
        [
          "test",
          "-p",
          "permark",
          "--lib",
          "plugins::tests::render_corpus",
          "--",
          "--ignored",
          "--exact",
        ],
        SRC_TAURI,
        env,
      );
    });
    if (options.only.includes("security")) {
      const cases = options.quick ? "500" : "5000";
      await step(steps, `sanitizer properties (${cases} cases)`, () => {
        run(
          "cargo",
          ["test", "-p", "permark-core", "--lib", "format::markdown::sanitize::props"],
          SRC_TAURI,
          { ...env, PROPTEST_CASES: cases },
        );
      });
    }
    if (rendered) {
      await step(steps, "deep server", async () => {
        const server = await startMock(APP_DIR, DEEP_MODE);
        held.server = server;
        env["PERMARK_DEEP_URL"] = server.url;
        await warmUp(server.url);
        return server.url;
      });
    }
    if (held.server !== null) {
      const specs = options.only.map((layer) => `${layer}.spec.ts`);
      await step(steps, `browser specs (${specs.join(", ")})`, () => {
        const cli = path.join(APP_DIR, "node_modules", "@playwright", "test", "cli.js");
        const args = [cli, "test", "-c", path.join(DEEP_DIR, "playwright.config.ts"), ...specs];
        if (options.grep !== null) args.push("--grep", options.grep);
        // failures are read from the report below; the exit status only says something failed
        const result = spawnSync(process.execPath, args, { cwd: APP_DIR, env, stdio: "inherit" });
        const report = path.join(dir, "results.json");
        if (!existsSync(report)) throw new Error("Playwright wrote no report");
        const outcome = browserOutcome(JSON.parse(readFileSync(report, "utf8")));
        browser = outcome;
        const ran = outcome.passed + outcome.failed + outcome.flaky;
        // a spec that does not load runs nothing: Playwright says so only by its exit status
        if (ran === 0) throw new Error(`no browser test ran (Playwright exited ${result.status})`);
      });
    }
  } finally {
    held.server?.stop();
  }

  const pending = options.update
    ? collectPending(path.join(dir, "results"), baselines, before).length
    : 0;
  const facts = {
    stamp: name,
    steps,
    browser,
    corpus: corpusFacts(),
    pixels,
    pending,
    pendingDir: path.join(baselines, "pending"),
    report: path.join(dir, "report", "index.html"),
    update: options.update,
  };
  const summary = summaryMarkdown(facts);
  writeFileSync(path.join(dir, "summary.md"), summary);
  console.log(`\n${summary}\nsummary: ${path.join(dir, "summary.md")}`);
  return passed(facts) ? 0 : 1;
}

async function main(): Promise<number> {
  let options: Options;
  try {
    options = parseOptions(process.argv.slice(2));
  } catch (e) {
    if (!(e instanceof UsageError)) throw e;
    console.error(`${e.message}\n${USAGE}`);
    return 2;
  }
  return options.accept !== null ? acceptOnly(options.accept) : deepRun(options);
}

process.exitCode = await main();
