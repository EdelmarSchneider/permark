// Runs a scenario in a headless browser against the mock app and writes the report (spec: ux-probe.md §5, §7). Facts and
// flags, never judgments: the skill `permark-ux-review` judges.

import AxeBuilder from "@axe-core/playwright";
import {
  chromium,
  webkit,
  type Browser,
  type BrowserContext,
  type ElementHandle,
  type Page,
} from "@playwright/test";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { changeOf, flagsOf, type ActionKind, type Change, type Flag, type Thumb } from "./facts.ts";
import {
  collect,
  observeRead,
  observeStart,
  observeStop,
  type Observed,
  type Snapshot,
} from "./page.ts";
import { thumbs } from "./pixels.ts";
import { probed, stepName, type Engine, type Scenario, type Step } from "./scenario.ts";
import { fireDrop, seed, settle } from "./seed.ts";
import { startMock } from "./server.ts";

export const AXE_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];
const ACTION_WAIT = 5_000;
const QUIET_MS = 200;
const DEFAULT_WITHIN = 1_000;
/** Before a probed action the page's pending timers are run at once (the fake clock jumps this far): the app's own
 *  delayed work (marked read after 1 s, a glow fading after 3 s, a marker after 8 s) happens before the action, never
 *  inside its window where it would look like the action's effect. */
const DRAIN_MS = 10_000;

export type AxeViolation = {
  id: string;
  impact: string | null;
  help: string;
  nodes: number;
  targets: string[];
};

/** The option, tab or toggle is already the chosen one: activating it again is expected to change nothing. */
const SELECTED =
  '[aria-checked="true"], [aria-selected="true"], [aria-pressed="true"], [aria-current]:not([aria-current="false"])';

function isSelected(target: ElementHandle<Element>): Promise<boolean> {
  return target.evaluate((el, sel) => el.matches(sel), SELECTED);
}

export type SweepEntry = {
  name: string;
  selector: string;
  effect: Change["effect"];
  explained: boolean;
  /** Already the chosen option | tab | toggle: no flag when nothing changes. */
  selected?: boolean;
  error?: string;
};

export type StepReport = {
  index: number;
  name: string;
  action: Step;
  probed: boolean;
  error?: string;
  focus?: { before: Snapshot["focus"] | null; after: Snapshot["focus"] };
  change?: Change | null;
  regions?: Snapshot["regions"];
  paintsOver?: Snapshot["paintsOver"];
  covered?: Snapshot["covered"];
  scrollers?: (Snapshot["scrollers"][number] & { thumb: Thumb | null })[];
  axe?: AxeViolation[] | null;
  promised?: Snapshot["keyHints"];
  sweep?: SweepEntry[];
  viewport?: Snapshot["viewport"];
  theme?: Snapshot["theme"];
  flags: Flag[];
  screenshot?: string;
};

export type Report = {
  scenario: string;
  engine: Engine;
  browserVersion: string;
  viewport: Scenario["viewport"];
  theme: Scenario["theme"];
  motion: Scenario["motion"];
  locale: string;
  app: string;
  commit: string | null;
  url: string;
  startedAt: string;
  steps: StepReport[];
  errors: string[];
};

export type RunOptions = {
  scenario: Scenario;
  /** Folder for report.json + screenshots (made if missing). */
  out: string;
  /** App folder served in mock mode (its own node_modules). */
  appDir: string;
  /** A running mock server instead (no start | stop). */
  url?: string;
  axe: boolean;
  log?: (line: string) => void;
};

async function launch(engine: Engine): Promise<Browser> {
  // Playwright's headless Chromium hides scroll bars by default: the scroll bar facts need them drawn
  switch (engine) {
    case "chromium":
      return chromium.launch({ headless: true, ignoreDefaultArgs: ["--hide-scrollbars"] });
    case "msedge":
      return chromium.launch({
        channel: "msedge",
        headless: true,
        ignoreDefaultArgs: ["--hide-scrollbars"],
      });
    case "webkit":
      return webkit.launch({ headless: true });
    default:
      return engine satisfies never;
  }
}

async function openApp(context: BrowserContext, url: string, errors: string[]): Promise<Page> {
  const page = await context.newPage();
  // fake clock that flows like the real one, so pending timers can be run before an action (DRAIN_MS)
  await page.clock.install();
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(`console: ${msg.text().slice(0, 300)}`);
  });
  page.on("pageerror", (err) => errors.push(`page: ${String(err).slice(0, 300)}`));
  await page.goto(url);
  await page.locator("#app > *").first().waitFor({ state: "attached", timeout: 30_000 });
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await settle(page);
  return page;
}

/** Runs the page's pending timers now, then waits until it is quiet. */
async function drain(page: Page): Promise<void> {
  await settle(page);
  await page.clock.runFor(DRAIN_MS);
  await settle(page);
}

/** The element an action works on (focused element for keys). */
async function targetOf(page: Page, step: Step): Promise<ElementHandle<Element> | null> {
  if (
    step.do === "click" ||
    step.do === "dblclick" ||
    step.do === "hover" ||
    step.do === "focus" ||
    step.do === "drag" ||
    step.do === "drop"
  ) {
    const loc = page.locator(step.target).first();
    await loc.waitFor({ state: "visible", timeout: ACTION_WAIT });
    return loc.elementHandle();
  }
  if (step.do === "press" && step.target !== undefined) {
    await page.locator(step.target).first().focus({ timeout: ACTION_WAIT });
  }
  const handle = await page.evaluateHandle(() => document.activeElement);
  return handle.asElement();
}

/** Does the step (no probing). */
async function perform(page: Page, step: Step): Promise<void> {
  switch (step.do) {
    case "seed":
      return seed(page, step.use, step.args);
    case "click": {
      // a control that is off but still says why (`aria-disabled`, focusable) can be clicked by a person: so here too
      const loc = page.locator(step.target).first();
      const off = (await loc.getAttribute("aria-disabled", { timeout: ACTION_WAIT })) === "true";
      return loc.click({ timeout: ACTION_WAIT, force: off });
    }
    case "dblclick":
      return page.locator(step.target).first().dblclick({ timeout: ACTION_WAIT });
    case "hover":
      return page.locator(step.target).first().hover({ timeout: ACTION_WAIT });
    case "focus":
      return page.locator(step.target).first().focus({ timeout: ACTION_WAIT });
    case "press":
      if (step.target !== undefined)
        await page.locator(step.target).first().focus({ timeout: ACTION_WAIT });
      return page.keyboard.press(step.key);
    case "type":
      return page.keyboard.type(step.text);
    case "drag":
    case "drop": {
      const box = await page.locator(step.target).first().boundingBox({ timeout: ACTION_WAIT });
      if (box === null) throw new Error(`${step.do}: ${step.target} not on screen`);
      const at = { x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2) };
      const paths = [...step.paths];
      await fireDrop(page, { type: "over", count: paths.length, paths, ...at });
      if (step.do === "drag") return;
      // the system lets go a moment after the last move
      await page.waitForTimeout(50);
      return fireDrop(page, { type: "drop", paths, ...at });
    }
    case "wait":
      if (step.target !== undefined) {
        await page.locator(step.target).first().waitFor({ state: "visible", timeout: ACTION_WAIT });
      }
      if (step.ms !== undefined) await page.waitForTimeout(step.ms);
      return;
    case "viewport":
      return page.setViewportSize({ width: step.width, height: step.height });
    case "probe":
    case "sweep":
      return;
    default:
      return step satisfies never;
  }
}

/** Runs `act`, then records the DOM until quiet (200 ms after the last change) or `within` ms with none. */
async function observe(
  page: Page,
  target: ElementHandle<Element> | null,
  within: number,
  act: () => Promise<void>,
): Promise<{ observed: Observed; error?: string }> {
  await page.evaluate(observeStart, target);
  const start = Date.now();
  let error: string | undefined;
  try {
    await act();
  } catch (e) {
    error = String(e instanceof Error ? e.message : e).split("\n")[0];
  }
  try {
    for (;;) {
      const elapsed = Date.now() - start;
      const seen = await page.evaluate(observeRead);
      if (seen.lastAtMs !== null && elapsed - seen.lastAtMs >= QUIET_MS) break;
      if (elapsed >= within) break;
      await page.waitForTimeout(50);
    }
    const observed = await page.evaluate(observeRead);
    return error === undefined ? { observed } : { observed, error };
  } finally {
    await page.evaluate(observeStop);
  }
}

const CONTROLS =
  'button:not([disabled]), a[href], [role="button"]:not([aria-disabled="true"]), [role="tab"], [role="radio"], [role="menuitem"], [role="checkbox"], summary';

/** Visible enabled controls in `scope`, in page order. */
async function controlsIn(
  page: Page,
  scope: string,
): Promise<{ name: string; selector: string }[]> {
  return page.evaluate(
    ({ scope, sel }) => {
      const root = document.querySelector(scope);
      if (!root) return [];
      return [...root.querySelectorAll(sel)]
        .filter((el) => {
          const r = el.getBoundingClientRect();
          return r.width > 0 && r.height > 0 && !el.closest("[inert]");
        })
        .map((el) => ({
          name: (el.getAttribute("aria-label") ?? (el as HTMLElement).innerText ?? "")
            .trim()
            .slice(0, 60),
          selector:
            el.tagName.toLowerCase() +
            (el.className ? `.${String(el.className).split(" ")[0]}` : ""),
        }));
    },
    { scope, sel: CONTROLS },
  );
}

async function axeOf(page: Page): Promise<AxeViolation[] | null> {
  try {
    const result = await new AxeBuilder({ page }).withTags(AXE_TAGS).analyze();
    return result.violations.map((v) => ({
      id: v.id,
      impact: v.impact ?? null,
      help: v.help,
      nodes: v.nodes.length,
      targets: v.nodes.slice(0, 3).map((n) => n.target.join(" ")),
    }));
  } catch {
    return null;
  }
}

function slug(text: string): string {
  return (
    text
      .replace(/[^a-zA-Z0-9-]+/g, "-")
      .replace(/^-|-$/g, "")
      .slice(0, 40) || "step"
  );
}

function commitOf(appDir: string): string | null {
  const r = spawnSync("git", ["-C", appDir, "rev-parse", "--short", "HEAD"], { encoding: "utf8" });
  return r.status === 0 ? r.stdout.trim() : null;
}

function actionKind(step: Step): ActionKind {
  if (step.do === "click" || step.do === "dblclick") return "click";
  if (step.do === "press") return "press";
  if (step.do === "probe") return "none";
  return "other";
}

/** Runs the scenario; the report is also written to `out/report.json`. */
export async function runScenario(opts: RunOptions): Promise<Report> {
  const { scenario } = opts;
  const log = opts.log ?? (() => {});
  mkdirSync(opts.out, { recursive: true });
  const server = opts.url === undefined ? await startMock(opts.appDir) : null;
  const url = opts.url ?? server?.url ?? "";
  const errors: string[] = [];
  const steps: StepReport[] = [];
  let browser: Browser | null = null;
  try {
    browser = await launch(scenario.engine);
    const contextOptions = {
      viewport: scenario.viewport,
      colorScheme: scenario.theme,
      locale: scenario.locale,
      reducedMotion:
        scenario.motion === "reduce" ? ("reduce" as const) : ("no-preference" as const),
    };
    const context = await browser.newContext(contextOptions);
    const page = await openApp(context, url, errors);
    for (const [i, step] of scenario.steps.entries()) {
      const index = i + 1;
      const name = stepName(step, index);
      if (step.do === "sweep") {
        steps.push(
          await sweep(
            browser,
            contextOptions,
            url,
            scenario.steps.slice(0, i),
            page,
            step,
            index,
            name,
          ),
        );
        log(`${String(index).padStart(2, "0")} ${name}: sweep`);
        continue;
      }
      if (!probed(step)) {
        const entry: StepReport = { index, name, action: step, probed: false, flags: [] };
        try {
          await perform(page, step);
          await settle(page);
        } catch (e) {
          entry.error = String(e instanceof Error ? e.message : e).split("\n")[0];
          steps.push(entry);
          log(`${String(index).padStart(2, "0")} ${name}: ERROR ${entry.error}`);
          break;
        }
        steps.push(entry);
        continue;
      }
      const watch = step.watch ?? null;
      await page.mouse.move(0, 0);
      await drain(page);
      const before = step.do === "probe" ? null : await page.evaluate(collect, { watch });
      let observed: Observed | null = null;
      let error: string | undefined;
      let selected = false;
      if (step.do !== "probe") {
        let target: ElementHandle<Element> | null = null;
        try {
          target = await targetOf(page, step);
          selected = target !== null && (await isSelected(target));
        } catch (e) {
          error = String(e instanceof Error ? e.message : e).split("\n")[0];
        }
        if (error === undefined) {
          const result = await observe(page, target, step.within ?? DEFAULT_WITHIN, () =>
            perform(page, step),
          );
          observed = result.observed;
          error = result.error;
        }
      }
      await page.mouse.move(0, 0);
      const after = await page.evaluate(collect, { watch });
      const change =
        before !== null && observed !== null ? changeOf(before, after, observed) : null;
      const laneThumbs = await thumbs(page, context, after.scrollers);
      const axe = opts.axe && step.axe !== false ? await axeOf(page) : null;
      const shot = `${String(index).padStart(2, "0")}-${slug(name)}.png`;
      await page.screenshot({
        path: path.join(opts.out, shot),
        animations: "disabled",
        caret: "hide",
      });
      const flags = error
        ? [{ id: "step-error", detail: error }]
        : flagsOf({
            action: actionKind(step),
            ...(step.do === "press" ? { key: step.key } : {}),
            before,
            after,
            change,
            observed,
            thumbs: laneThumbs,
            axe: axe ?? [],
            expectChange: step.expectChange === true,
            selected,
          });
      steps.push({
        index,
        name,
        action: step,
        probed: true,
        ...(error ? { error } : {}),
        focus: { before: before?.focus ?? null, after: after.focus },
        change,
        regions: after.regions,
        paintsOver: after.paintsOver,
        covered: after.covered,
        scrollers: after.scrollers.map((s) => ({
          ...s,
          thumb: laneThumbs.find((t) => t.selector === s.selector) ?? null,
        })),
        axe,
        promised: step.do === "press" ? after.keyHints : [],
        viewport: after.viewport,
        theme: after.theme,
        flags,
        screenshot: shot,
      });
      log(`${String(index).padStart(2, "0")} ${name}: ${flags.map((f) => f.id).join(" ") || "-"}`);
      if (error && step.do !== "probe") break;
    }
    const report: Report = {
      scenario: scenario.name,
      engine: scenario.engine,
      browserVersion: browser.version(),
      viewport: scenario.viewport,
      theme: scenario.theme,
      motion: scenario.motion,
      locale: scenario.locale,
      app: opts.appDir,
      commit: commitOf(opts.appDir),
      url,
      startedAt: new Date().toISOString(),
      steps,
      errors,
    };
    writeFileSync(path.join(opts.out, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
    return report;
  } finally {
    await browser?.close();
    server?.stop();
  }
}

/** Every control of the scope, each on a fresh page at the same state: did activating it change anything? */
async function sweep(
  browser: Browser,
  contextOptions: Parameters<Browser["newContext"]>[0],
  url: string,
  prior: Step[],
  page: Page,
  step: Extract<Step, { do: "sweep" }>,
  index: number,
  name: string,
): Promise<StepReport> {
  const controls = (await controlsIn(page, step.target)).slice(0, step.max ?? 12);
  const entries: SweepEntry[] = [];
  for (const [k, control] of controls.entries()) {
    const context = await browser.newContext(contextOptions);
    try {
      const fresh = await openApp(context, url, []);
      for (const s of prior) {
        if (s.do === "sweep" || s.do === "probe") continue;
        await perform(fresh, s);
        await settle(fresh);
      }
      // the CONTROLS selector matches elements only
      const handles = (await fresh
        .locator(step.target)
        .first()
        .locator(CONTROLS)
        .elementHandles()) as ElementHandle<Element>[];
      const visibleHandles: ElementHandle<Element>[] = [];
      for (const h of handles) if (await h.isVisible()) visibleHandles.push(h);
      const target = visibleHandles[k] ?? null;
      if (target === null) {
        entries.push({
          ...control,
          effect: "none",
          explained: false,
          error: "control not found on replay",
        });
        continue;
      }
      await drain(fresh);
      const selected = await isSelected(target);
      const liveBefore = await fresh.evaluate(collect, { watch: null });
      const { observed, error } = await observe(fresh, target, step.within ?? DEFAULT_WITHIN, () =>
        target.click({ timeout: ACTION_WAIT }),
      );
      const liveAfter = await fresh.evaluate(collect, { watch: null });
      const change = changeOf(liveBefore, liveAfter, observed);
      entries.push({
        ...control,
        effect: change.effect,
        explained: change.explained,
        ...(selected ? { selected } : {}),
        ...(error ? { error } : {}),
      });
    } catch (e) {
      entries.push({
        ...control,
        effect: "none",
        explained: false,
        error: String(e).split("\n")[0],
      });
    } finally {
      await context.close();
    }
  }
  const flags: Flag[] = [];
  for (const e of entries) {
    if (e.error !== undefined || e.explained || e.selected === true) continue;
    if (e.effect === "none")
      flags.push({ id: "dead-control", detail: `"${e.name}" (${e.selector}) changed nothing` });
    if (e.effect === "selfOnly") {
      flags.push({
        id: "no-effect-beyond-control",
        detail: `"${e.name}" (${e.selector}) changed only itself`,
      });
    }
  }
  return { index, name, action: step, probed: true, sweep: entries, flags };
}
