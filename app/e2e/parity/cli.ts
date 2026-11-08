// `npm run ux:parity -- [--doc <path>]... [--theme light|dark] [--url <running mock url>] [--out <report.json>] [--all]`
// Opens the mock showcase documents in View, then in Edit (light + dark), and compares computed styles and block
// geometry. Exit 0 = every difference is a known gap, 1 = a difference not in `known-gaps.json`, 2 = usage | setup.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  compareSides,
  entryDiffs,
  groupDiffs,
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
/** The header's mode switch: View, Edit, Source in this order. */
const MODE = ".modes button.mode";
const WAIT = 15_000;
/** Past the glow a live change puts on blocks for a moment (3 s): the comparison is of the page at rest. */
const AT_REST_MS = 3_500;
/** Longest value shown in the table (the JSON report keeps the whole value). */
const CELL = 44;

const USAGE = `usage: npm run ux:parity -- [--doc <path>]... [--theme light|dark] [--url <running mock url>]
  [--out <report.json>] [--all]
  --all: list the known gaps too (default: new differences only, known ones counted per phase)`;

type Args = {
  docs: string[];
  themes: Theme[];
  url?: string;
  out: string;
  all: boolean;
  /** Some documents | one theme only: a known gap not seen may just not have been looked at. */
  partial: boolean;
};

function parseArgs(argv: string[]): Args {
  const docs: string[] = [];
  let themes = THEMES;
  let url: string | undefined;
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
  return {
    docs: docs.length > 0 ? docs : DOCS,
    themes,
    ...(url ? { url } : {}),
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

function table(groups: DiffGroup[], phaseOf?: (g: DiffGroup) => string): string {
  const head = ["doc", "theme", "element#", "property", "View", "Edit"];
  if (phaseOf) head.push("phase");
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
    if (phaseOf) row.push(phaseOf(g));
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

/** Every document in every theme: View first, then Edit through the header's mode switch, then back to View. */
async function compareAll(
  { run, seed, snapshot }: Awaited<ReturnType<typeof loadBrowserSide>>,
  url: string,
  args: Args,
  errors: string[],
): Promise<Diff[]> {
  const diffs: Diff[] = [];
  const browser = await run.launch("chromium");
  try {
    for (const theme of args.themes) {
      const context = await browser.newContext({
        viewport: VIEWPORT,
        colorScheme: theme,
        locale: "en-US",
        reducedMotion: "reduce",
      });
      try {
        const page = await run.openApp(context, url, errors);
        await seed.seed(page, "project", {});
        for (const doc of args.docs) {
          const where = { doc, theme };
          await seed.seed(page, "doc", { path: doc });
          await seed.settle(page);
          await page.clock.runFor(AT_REST_MS);
          await seed.settle(page);
          const view = await page.evaluate(snapshot.sideOf, VIEW_SIDE);
          if (view === null) throw new Error(`${doc}: no reading view (${VIEW_ROOT})`);
          const viewScroll = await page.evaluate(snapshot.scrollOf);
          await page.locator(MODE).nth(1).click({ timeout: WAIT });
          await page.locator(EDIT_ROOT).waitFor({ state: "visible", timeout: WAIT });
          await seed.settle(page);
          const entry = await page.evaluate(snapshot.entryOf);
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
          await page.locator(MODE).nth(0).click({ timeout: WAIT });
          await page.locator(".reading:not([hidden])").waitFor({ state: "visible", timeout: WAIT });
          await seed.settle(page);
          console.log(`${theme} ${doc}: ${view.blocks.length} | ${edit.blocks.length} blocks`);
        }
      } finally {
        await context.close();
      }
    }
  } finally {
    await browser.close();
  }
  return diffs;
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
  const errors: string[] = [];
  const started = Date.now();
  let url = args.url ?? "";
  try {
    gaps = parseKnownGaps(JSON.parse(readFileSync(path.join(HERE, "known-gaps.json"), "utf8")));
    const browserSide = await loadBrowserSide();
    const mock = args.url === undefined ? await browserSide.server.startMock(APP) : null;
    url = args.url ?? mock?.url ?? "";
    try {
      diffs = await compareAll(browserSide, url, args, errors);
    } finally {
      mock?.stop();
    }
  } catch (e) {
    console.error(`parity failed: ${e instanceof Error ? (e.stack ?? e.message) : String(e)}`);
    return 2;
  }

  const result = triage(diffs, gaps);
  const phaseOf = new Map(result.covered.map((c) => [c.diff, c.gap.phase]));
  // one document's rows together (a theme-only row would otherwise trail after every document)
  const byDoc = (a: DiffGroup, b: DiffGroup): number =>
    args.docs.indexOf(a.doc) - args.docs.indexOf(b.doc);
  const fresh = groupDiffs(result.uncovered).sort(byDoc);
  const known = groupDiffs(result.covered.map((c) => c.diff)).sort(byDoc);
  const perPhase = new Map<string, number>();
  for (const c of result.covered) perPhase.set(c.gap.phase, (perPhase.get(c.gap.phase) ?? 0) + 1);
  mkdirSync(path.dirname(args.out), { recursive: true });
  writeFileSync(
    args.out,
    `${JSON.stringify(
      {
        url,
        viewport: VIEWPORT,
        docs: args.docs,
        themes: args.themes,
        seconds: Math.round((Date.now() - started) / 100) / 10,
        diffs: diffs.map((d) => ({ ...d, phase: phaseOf.get(d) ?? null })),
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
    const firstPhase = new Map(
      result.covered.map((c) => [
        `${c.diff.doc}|${c.diff.element}|${c.diff.property}`,
        c.gap.phase,
      ]),
    );
    console.log("\nknown gaps:");
    console.log(table(known, (g) => firstPhase.get(`${g.doc}|${g.element}|${g.property}`) ?? ""));
  }
  const counts = [...perPhase].map(([p, n]) => `${p} ${n}`).join(", ");
  console.log(`\nknown: ${result.covered.length} (${counts || "none"})`);
  if (args.partial) {
    console.log("partial run: known gaps not checked for staleness");
  } else if (result.stale.length > 0) {
    console.log("known gaps no longer seen (remove them from known-gaps.json):");
    for (const g of result.stale)
      console.log(`  ${g.phase}: ${g.element} ${g.property} (${g.doc})`);
  }
  if (errors.length > 0) console.log(`page errors: ${errors.length} (report errors[])`);
  console.log(`report: ${args.out} (${Math.round((Date.now() - started) / 1000)} s)`);
  return result.uncovered.length > 0 ? 1 : 0;
}

process.exitCode = await main();
