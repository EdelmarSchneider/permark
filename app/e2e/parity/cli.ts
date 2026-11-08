// `npm run ux:parity -- [--doc <path>]... [--theme light|dark] [--url <running mock url> | --cdp <app url> |
//   --app <exe|debug> [--project <dir>]] [--out <report.json>] [--all]`
// Opens the showcase documents in View, then in Edit (light + dark), and compares computed styles and block geometry:
// in the mock app (default) or in the real app (`--cdp`, `--app`). Exit 0 = every difference is a known gap, 1 = a
// difference not in `known-gaps.json`, 2 = usage | setup.

import type { Page } from "@playwright/test";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  compareSides,
  entryDiffs,
  groupDiffs,
  openCardDiffs,
  parseKnownGaps,
  triage,
  type Diff,
  type DiffGroup,
  type KnownGap,
  type Theme,
} from "./compare.ts";
import { loopbackUrl, UsageError } from "../probe/options.ts";
import { oneOf } from "../probe/scenario.ts";
import type { SideArg } from "./snapshot.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const APP = path.resolve(HERE, "..", "..");

/** The showcase documents of the mock project: every block kind the reading view draws. */
const DOCS = ["README.md", "docs/guide.md", "docs/handbook.md", "CLAUDE.md"];
const THEMES: Theme[] = ["light", "dark"];
const VIEWPORT = { width: 1200, height: 900 };
const VIEW_ROOT = ".reading .md-body";
const EDIT_ROOT = ".pm-editor";
const VIEW_SIDE: SideArg = { root: VIEW_ROOT, mode: "view" };
const EDIT_SIDE: SideArg = { root: EDIT_ROOT, mode: "edit" };
/** Edit's front matter at rest (View's card) and opened for editing (its properties). */
const CARD = `${EDIT_ROOT} > .pm-locked[data-look="card"]`;
const OPEN_CARD = `${EDIT_ROOT} > .pm-locked[data-look="properties"]`;
/** The header's mode switch: View, Edit, Source in this order. */
const MODE = ".modes button.mode";
const WAIT = 15_000;
/** Past the glow a live change puts on blocks for a moment (3 s): the comparison is of the page at rest. */
const AT_REST_MS = 3_500;
/** Longest value shown in the table (the JSON report keeps the whole value). */
const CELL = 44;

const USAGE = `usage: npm run ux:parity -- [--doc <path>]... [--theme light|dark] [--out <report.json>] [--all]
  [--url <running mock url> | --cdp <app url> | --app <exe|debug> [--project <dir>]]
  --url: a running dev:mock (default: one started for the run)
  --cdp: a running PerMark started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>,
         its project holding the documents (http://127.0.0.1:<port>)
  --app: starts this build (debug: the debug build in CARGO_TARGET_DIR) on a temp copy of the fixture documents
         (or on --project), compares, then closes it and deletes the copy
  --all: list the known gaps too (default: new differences only, known ones counted per area)`;

type Args = {
  docs: string[];
  themes: Theme[];
  url?: string;
  cdp?: string;
  app?: string;
  project?: string;
  out: string;
  all: boolean;
  /** Some documents | one theme only: a known gap not seen may just not have been looked at. */
  partial: boolean;
};

/** `debug`: the debug build where cargo puts it (the shared target folder when set). */
function exeOf(value: string): string {
  if (value !== "debug") return path.resolve(value);
  const target = process.env["CARGO_TARGET_DIR"] ?? path.join(APP, "src-tauri", "target");
  return path.join(target, "debug", process.platform === "win32" ? "permark.exe" : "permark");
}

function parseArgs(argv: string[]): Args {
  const docs: string[] = [];
  let themes = THEMES;
  let url: string | undefined;
  let cdp: string | undefined;
  let app: string | undefined;
  let project: string | undefined;
  let out = path.join(tmpdir(), "permark-ux-parity", "report.json");
  let all = false;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i] ?? "";
    const value = (): string => {
      const v = argv[++i];
      if (v === undefined || v.startsWith("--")) throw new UsageError(`${a}: value expected`);
      return v;
    };
    switch (a) {
      case "--doc":
        docs.push(value());
        break;
      case "--theme":
        themes = [oneOf(value(), THEMES, a, "light")];
        break;
      case "--url":
        url = loopbackUrl(value());
        break;
      case "--cdp":
        cdp = loopbackUrl(`${value().replace(/\/+$/, "")}/`).replace(/\/$/, "");
        break;
      case "--app":
        app = exeOf(value());
        if (!existsSync(app)) throw new UsageError(`--app: no build at ${app}`);
        break;
      case "--project":
        project = path.resolve(value());
        break;
      case "--out":
        out = path.resolve(value());
        break;
      case "--all":
        all = true;
        break;
      default:
        throw new UsageError(`unknown option ${a}`);
    }
  }
  if ([url, cdp, app].filter((x) => x !== undefined).length > 1) {
    throw new UsageError("--url, --cdp, --app: one at most");
  }
  if (project !== undefined && app === undefined)
    throw new UsageError("--project: with --app only");
  return {
    docs: docs.length > 0 ? docs : DOCS,
    themes,
    ...(url ? { url } : {}),
    ...(cdp ? { cdp } : {}),
    ...(app ? { app } : {}),
    ...(project ? { project } : {}),
    out,
    all,
    partial: docs.length > 0 || themes !== THEMES,
  };
}

function ranges(indexes: number[]): string {
  const parts: string[] = [];
  for (let i = 0; i < indexes.length;) {
    let j = i;
    while (j + 1 < indexes.length && indexes[j + 1] === (indexes[j] ?? 0) + 1) j++;
    parts.push(i === j ? `${indexes[i]}` : `${indexes[i]}-${indexes[j]}`);
    i = j + 1;
  }
  return parts.length > 4 ? `${parts.slice(0, 4).join(",")},..` : parts.join(",");
}

function cell(v: string): string {
  return v.length > CELL ? `${v.slice(0, CELL - 1)}…` : v;
}

function table(groups: DiffGroup[], areaOf?: (g: DiffGroup) => string): string {
  const head = ["doc", "theme", "element#", "property", "View", "Edit"];
  if (areaOf) head.push("area");
  const rows = groups.map((g) => {
    const times = g.count > g.indexes.length * g.themes.length ? ` x${g.count}` : "";
    const delta = g.delta === null ? "" : ` (${g.delta > 0 ? "+" : ""}${g.delta})`;
    const row = [
      g.doc,
      g.themes.join("+"),
      `${g.element} #${ranges(g.indexes)}${times}`,
      g.property,
      cell(g.view),
      cell(g.edit) + delta,
    ];
    if (areaOf) row.push(areaOf(g));
    return row;
  });
  const widths = head.map((h, c) => Math.max(h.length, ...rows.map((r) => (r[c] ?? "").length)));
  const line = (r: string[]): string =>
    r
      .map((v, c) => v.padEnd(widths[c] ?? 0))
      .join("  ")
      .trimEnd();
  return [line(head), ...rows.map(line)].join("\n");
}

/** The browser half: needs the dev dependencies (Playwright). */
async function loadBrowserSide() {
  try {
    return {
      app: await import("../probe/app.ts"),
      run: await import("../probe/run.ts"),
      seed: await import("../probe/seed.ts"),
      server: await import("../probe/server.ts"),
      snapshot: await import("./snapshot.ts"),
    };
  } catch (e) {
    throw new Error(
      `parity dependencies missing (${e instanceof Error ? e.message : String(e)}): run \`npm ci\` in app/ first`,
    );
  }
}

type BrowserSide = Awaited<ReturnType<typeof loadBrowserSide>>;

/** The app in one theme, its project open: `rest` lets its timers run (the mock's fake clock | real time). */
type ThemePage = { page: Page; rest(ms: number): Promise<void>; close(): Promise<void> };
type Target = { open(theme: Theme): Promise<ThemePage>; close(): Promise<void> };

/** The mock app at `url`: one fresh headless page per theme. */
async function mockTarget(side: BrowserSide, url: string, errors: string[]): Promise<Target> {
  const browser = await side.run.launch("chromium");
  return {
    open: async (theme) => {
      const context = await browser.newContext({
        viewport: VIEWPORT,
        colorScheme: theme,
        locale: "en-US",
        reducedMotion: "reduce",
      });
      try {
        const page = await side.run.openApp(context, url, errors);
        await side.seed.seed(page, "project", {});
        return { page, rest: (ms) => page.clock.runFor(ms), close: () => context.close() };
      } catch (e) {
        await context.close();
        throw e;
      }
    },
    close: () => browser.close(),
  };
}

/** The real app at `cdpUrl`: its project view in each theme through its theme setting, put back when done. */
async function realTarget(side: BrowserSide, cdpUrl: string, errors: string[]): Promise<Target> {
  const app = await side.app.connectApp(cdpUrl, errors);
  const { page } = app;
  let before: string | null = null;
  return {
    open: async (theme) => {
      await page.emulateMedia({ reducedMotion: "reduce" });
      const was = await side.app.setTheme(page, theme, theme);
      before ??= was;
      await side.seed.settle(page);
      return { page, rest: (ms) => page.waitForTimeout(ms), close: async () => {} };
    },
    close: async () => {
      try {
        if (before !== null) await side.app.setTheme(page, before, null);
        await page.emulateMedia({ reducedMotion: null });
      } finally {
        await app.close();
      }
    },
  };
}

/** Every document in every theme: View first, then Edit through the header's mode switch, then back to View. */
async function compareAll(
  { seed, snapshot }: BrowserSide,
  target: Target,
  args: Args,
): Promise<{ diffs: Diff[]; viewport: { width: number; height: number } }> {
  const diffs: Diff[] = [];
  let viewport = VIEWPORT;
  for (const theme of args.themes) {
    const { page, rest, close } = await target.open(theme);
    try {
      viewport = await viewportOf(page);
      // every block comes near the screen once, then the pane is back at the top and at rest: lazy embeds (math,
      // diagrams) are drawn in both modes wherever they are, so both are compared fully drawn
      const drawAll = async (): Promise<void> => {
        const screens = await page.evaluate(snapshot.screensOf);
        for (let screen = 1; screen < screens; screen++) {
          await page.evaluate(snapshot.scrollToScreen, screen);
          await seed.settle(page, 100);
        }
        await page.evaluate(snapshot.scrollToScreen, 0);
        await rest(AT_REST_MS);
        await seed.settle(page);
      };
      for (const doc of args.docs) {
        const where = { doc, theme };
        await seed.seed(page, "doc", { path: doc });
        await seed.settle(page);
        await rest(AT_REST_MS);
        await seed.settle(page);
        await drawAll();
        const view = await page.evaluate(snapshot.sideOf, VIEW_SIDE);
        if (view === null) throw new Error(`${doc}: no reading view (${VIEW_ROOT})`);
        const viewCard = await page.evaluate(snapshot.cardOf, VIEW_SIDE);
        const viewScroll = await page.evaluate(snapshot.scrollOf);
        await page.locator(MODE).nth(1).click({ timeout: WAIT });
        await page.locator(EDIT_ROOT).waitFor({ state: "visible", timeout: WAIT });
        await seed.settle(page);
        const entry = await page.evaluate(snapshot.entryOf);
        await drawAll();
        const edit = await page.evaluate(snapshot.sideOf, EDIT_SIDE);
        if (edit === null) throw new Error(`${doc}: no visual editor (${EDIT_ROOT})`);
        diffs.push(
          ...entryDiffs(where, {
            viewScroll,
            editScroll: entry.scroll,
            selected: entry.selected,
            toolbarRows: entry.toolbarRows,
          }),
          ...compareSides(where, view, edit),
        );
        if (viewCard !== null) {
          // the front matter opened for editing still reads as View's card
          await page.locator(CARD).click({ timeout: WAIT });
          const open = await page
            .locator(`${OPEN_CARD} .sheet`)
            .waitFor({ state: "visible", timeout: WAIT })
            .then(() => page.evaluate(snapshot.cardOf, EDIT_SIDE))
            .catch(() => null);
          diffs.push(...openCardDiffs(where, viewCard, open));
          await page.keyboard.press("Escape");
          await seed.settle(page);
        }
        await page.locator(MODE).nth(0).click({ timeout: WAIT });
        await page.locator(".reading:not([hidden])").waitFor({ state: "visible", timeout: WAIT });
        await seed.settle(page);
        console.log(`${theme} ${doc}: ${view.blocks.length} | ${edit.blocks.length} blocks`);
      }
    } finally {
      await close();
    }
  }
  return { diffs, viewport };
}

/** The app the run compares in, started for it when needed; `stop` undoes what was started. */
async function targetOf(
  side: BrowserSide,
  args: Args,
  errors: string[],
): Promise<{ target: Target; where: string; stop(): void }> {
  const started: (() => void)[] = [];
  const stop = (): void => {
    for (const undo of started.reverse()) undo();
  };
  try {
    if (args.app !== undefined) {
      let project = args.project;
      if (project === undefined) {
        const copy = side.app.fixtureProject(APP);
        started.push(copy.remove);
        project = copy.dir;
      }
      const app = await side.app.startApp(args.app, project);
      started.push(app.stop);
      const target = await realTarget(side, app.cdpUrl, errors);
      return { target, where: `${args.app} ${project}`, stop };
    }
    if (args.cdp !== undefined) {
      return { target: await realTarget(side, args.cdp, errors), where: args.cdp, stop };
    }
    let url = args.url;
    if (url === undefined) {
      const mock = await side.server.startMock(APP);
      started.push(mock.stop);
      url = mock.url;
    }
    return { target: await mockTarget(side, url, errors), where: url, stop };
  } catch (e) {
    stop();
    throw e;
  }
}

/** The view's size in CSS pixels (the real app's is its window's). */
function viewportOf(page: Page): Promise<{ width: number; height: number }> {
  return page.evaluate(() => ({ width: window.innerWidth, height: window.innerHeight }));
}

async function main(): Promise<number> {
  let args: Args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    console.error(USAGE);
    return 2;
  }
  let diffs: Diff[];
  let gaps: KnownGap[];
  let viewport = VIEWPORT;
  const errors: string[] = [];
  const started = Date.now();
  let url = "";
  try {
    gaps = parseKnownGaps(JSON.parse(readFileSync(path.join(HERE, "known-gaps.json"), "utf8")));
    const browserSide = await loadBrowserSide();
    const run = await targetOf(browserSide, args, errors);
    url = run.where;
    try {
      ({ diffs, viewport } = await compareAll(browserSide, run.target, args));
    } finally {
      try {
        await run.target.close();
      } finally {
        run.stop();
      }
    }
  } catch (e) {
    console.error(`parity failed: ${e instanceof Error ? (e.stack ?? e.message) : String(e)}`);
    return 2;
  }

  const result = triage(diffs, gaps);
  const areaOf = new Map(result.covered.map((c) => [c.diff, c.gap.area]));
  // one document's rows together (a theme-only row would otherwise trail after every document)
  const byDoc = (a: DiffGroup, b: DiffGroup): number =>
    args.docs.indexOf(a.doc) - args.docs.indexOf(b.doc);
  const fresh = groupDiffs(result.uncovered).sort(byDoc);
  const known = groupDiffs(result.covered.map((c) => c.diff)).sort(byDoc);
  const perArea = new Map<string, number>();
  for (const c of result.covered) perArea.set(c.gap.area, (perArea.get(c.gap.area) ?? 0) + 1);
  mkdirSync(path.dirname(args.out), { recursive: true });
  writeFileSync(
    args.out,
    `${JSON.stringify(
      {
        url,
        viewport,
        docs: args.docs,
        themes: args.themes,
        seconds: Math.round((Date.now() - started) / 100) / 10,
        diffs: diffs.map((d) => ({ ...d, area: areaOf.get(d) ?? null })),
        stale: args.partial ? null : result.stale,
        errors,
      },
      null,
      2,
    )}\n`,
  );

  console.log("");
  if (fresh.length > 0) {
    console.log(
      `NEW differences (not in known-gaps.json): ${result.uncovered.length} in ${fresh.length} rows`,
    );
    console.log(table(fresh));
  } else {
    console.log("NEW differences: none");
  }
  if (args.all && known.length > 0) {
    const firstArea = new Map(
      result.covered.map((c) => [`${c.diff.doc}|${c.diff.element}|${c.diff.property}`, c.gap.area]),
    );
    console.log("\nknown gaps:");
    console.log(table(known, (g) => firstArea.get(`${g.doc}|${g.element}|${g.property}`) ?? ""));
  }
  const counts = [...perArea].map(([p, n]) => `${p} ${n}`).join(", ");
  console.log(`\nknown: ${result.covered.length} (${counts || "none"})`);
  if (args.partial) {
    console.log("partial run: known gaps not checked for staleness");
  } else if (result.stale.length > 0) {
    console.log("known gaps no longer seen (remove them from known-gaps.json):");
    for (const g of result.stale) console.log(`  ${g.area}: ${g.element} ${g.property} (${g.doc})`);
  }
  if (errors.length > 0) console.log(`page errors: ${errors.length} (report errors[])`);
  console.log(`report: ${args.out} (${Math.round((Date.now() - started) / 1000)} s)`);
  return result.uncovered.length > 0 ? 1 : 0;
}

process.exitCode = await main();
