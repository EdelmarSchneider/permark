// The app page every deep spec drives: the deep server's app with its project open, guarded for the whole test.
// Guards (any one fails the test): a request off this machine (aborted), a Content Security Policy violation, a
// dialog, an uncaught page error, the attack canary `window.__pmCanary`.

import { test as base, expect, type Locator, type Page } from "@playwright/test";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import path from "node:path";
import { screensOf, scrollToScreen } from "../parity/snapshot.ts";
import { openApp } from "../probe/run.ts";
import { settle } from "../probe/seed.ts";
import { DEEP_DIR, GOLDENS_DIR } from "./paths.ts";
import { serverUrl, UPDATE } from "./settings.ts";

/** Clock start of every page: dates and "x minutes ago" read the same on every run. */
const FIXED_TIME = Date.UTC(2026, 0, 15, 10, 0, 0);

/** Embeds that say they are still drawing (as the exporter waits for them). */
const BUSY = ".figure.is-pending, [aria-busy='true']";

/** The header's mode switch: View, Edit, Source in this order. */
const MODES = { view: 0, edit: 1, source: 2 } as const;
export type Mode = keyof typeof MODES;

const LOCAL_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

/** A request that stays on this machine (the deep server, inline data). */
export function isLocal(url: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  if (["data:", "blob:", "about:"].includes(parsed.protocol)) return true;
  return (
    ["http:", "https:", "ws:", "wss:"].includes(parsed.protocol) && LOCAL_HOSTS.has(parsed.hostname)
  );
}

/** The parts of the app's modules the specs reach, as the page loaded them (dev-server paths; the page's own
 *  instances, so the app sees what they do). Page functions cannot build code from text: the app's CSP forbids it. */
const MOCK_URL = "/src/api/mock.ts";
const DOCUMENTS_URL = "/src/core/doc/document.svelte.ts";

type MockModule = {
  MOCK_FOLDER: string;
  mockApi: {
    projectSessionSet(key: string, value: unknown): Promise<unknown>;
    openPath(path: string, placement: string): Promise<unknown>;
    showDocument(path: string): Promise<unknown>;
  };
};
type DocumentsModule = { documents: { decorator: { idle(): Promise<void> } } };

export class DeepApp {
  readonly page: Page;
  /** What went wrong so far (guards). */
  readonly problems: string[];
  /** Console errors (reported, not failing: a diagram engine logs the syntax errors it draws). */
  readonly console: string[];

  constructor(page: Page, problems: string[], console: string[]) {
    this.page = page;
    this.problems = problems;
    this.console = console;
  }

  /** The document pane (what a "first screen" shot takes). */
  get pane(): Locator {
    return this.page.locator("[data-scroller]").first();
  }

  /** The reading view's body. */
  get body(): Locator {
    return this.page.locator(".reading .md-body");
  }

  /** Opens the mock project with its side regions hidden: the document takes the window's width. */
  async openProject(): Promise<void> {
    await this.page.evaluate(async (url) => {
      const m = (await import(url)) as MockModule;
      await m.mockApi.projectSessionSet("layout.sidebar-hidden", true);
      await m.mockApi.projectSessionSet("layout.right-hidden", true);
      await m.mockApi.openPath(m.MOCK_FOLDER, "tab");
    }, MOCK_URL);
    await this.page.locator(".doc-header, .welcome, [data-scroller]").first().waitFor();
    await settle(this.page);
  }

  /** Shows the corpus page `path` as the Explorer does (the backend moves the tab's document). */
  async show(path: string): Promise<void> {
    await this.page.evaluate(
      async ({ url, path }) => {
        const m = (await import(url)) as MockModule;
        await m.mockApi.showDocument(path);
      },
      { url: MOCK_URL, path },
    );
    const name = path.split("/").at(-1) ?? path;
    await this.page.waitForFunction(
      (n) => document.querySelector(".doc-header")?.textContent?.includes(n) === true,
      name,
    );
    await this.body.waitFor({ state: "attached" });
    await this.drawn();
  }

  /** Waits until every embed started is loaded and drawn, the fonts they use are in, and the page is quiet. */
  async drawn(): Promise<void> {
    await this.page.evaluate(async (url) => {
      const m = (await import(url)) as DocumentsModule;
      await m.documents.decorator.idle();
      // a layout now starts the loads of the fonts the drawn embeds use (a formula's); table columns are measured
      // again once they are in
      document.body.getBoundingClientRect();
      await document.fonts.ready;
    }, DOCUMENTS_URL);
    await this.page.waitForFunction((busy) => document.querySelector(busy) === null, BUSY);
    await settle(this.page);
  }

  /** Brings every block near the screen once (embeds draw near the screen), then back to the top, all drawn. */
  async drawAll(): Promise<void> {
    const screens = await this.page.evaluate(screensOf);
    for (let screen = 1; screen < screens; screen++) {
      await this.page.evaluate(scrollToScreen, screen);
      await settle(this.page, 100);
    }
    await this.page.evaluate(scrollToScreen, 0);
    await this.drawn();
  }

  /** Scrolls the document pane back to its top. */
  async top(): Promise<void> {
    await this.page.evaluate(scrollToScreen, 0);
    await settle(this.page, 100);
  }

  /** Switches the document to `mode` through the header's switch. */
  async mode(mode: Mode): Promise<void> {
    await this.page.locator(".modes button.mode").nth(MODES[mode]).click();
    const shown = { view: ".reading:not([hidden])", edit: ".pm-editor", source: ".cm-editor" }[
      mode
    ];
    await this.page.locator(shown).first().waitFor({ state: "visible" });
    // the pointer rests where it hovers nothing (no tooltip, no hover look in a shot)
    await this.page.mouse.move(1, (this.page.viewportSize()?.height ?? 800) - 1);
    await settle(this.page);
  }

  /** The guards' findings so far (the page's own records included). */
  async findings(): Promise<string[]> {
    const page = await this.page
      .evaluate(() => ({
        csp: Reflect.get(window, "__pmCsp"),
        canary: Reflect.get(window, "__pmCanary"),
      }))
      .catch(() => ({ csp: undefined, canary: undefined }));
    const found = [...this.problems];
    if (Array.isArray(page.csp)) found.push(...page.csp.map((v) => `CSP violation: ${String(v)}`));
    if (page.canary !== undefined) found.push(`canary set: ${String(page.canary)}`);
    return found;
  }

  /** Findings so far, each named by the step that came before it (`checkpoint`). */
  readonly labeled: string[] = [];
  #seen = 0;

  /** Names the findings since the last checkpoint with `where` (the step just done). */
  async checkpoint(where: string): Promise<void> {
    const all = await this.findings();
    this.labeled.push(...all.slice(this.#seen).map((f) => `${where}: ${f}`));
    this.#seen = all.length;
  }

  /** Inline handlers seen while markup was parsed off the page (reported, not failing). */
  async inertReports(): Promise<string[]> {
    const inert: unknown = await this.page
      .evaluate(() => Reflect.get(window, "__pmCspInert"))
      .catch(() => []);
    return Array.isArray(inert) ? inert.map(String) : [];
  }
}

export const test = base.extend<{ app: DeepApp }>({
  app: async ({ context }, use, testInfo) => {
    const problems: string[] = [];
    const consoleErrors: string[] = [];
    await context.route(
      (url) => !isLocal(url.href),
      async (route) => {
        problems.push(`request off this machine: ${route.request().url()}`);
        await route.abort("blockedbyclient");
      },
    );
    await context.addInitScript(() => {
      const seen: string[] = [];
      const inert: string[] = [];
      Reflect.set(window, "__pmCsp", seen);
      Reflect.set(window, "__pmCspInert", inert);
      document.addEventListener("securitypolicyviolation", (e) => {
        const line = `${e.effectiveDirective} ${e.blockedURI || "inline"} (${e.sourceFile}:${e.lineNumber})`;
        // Chromium reports an inline handler already when markup holding one is parsed into a detached document
        // (a sanitizer's DOMParser | template): nothing can run there. On an element of the page it is a bypass.
        if (e.effectiveDirective === "script-src-attr" && !(e.target instanceof Element))
          inert.push(line);
        else seen.push(line);
      });
    });
    const errors: string[] = [];
    const page = await openApp(context, serverUrl(), errors, FIXED_TIME);
    page.on("dialog", (dialog) => {
      problems.push(`dialog: ${dialog.type()} ${dialog.message()}`);
      void dialog.dismiss();
    });
    const app = new DeepApp(page, problems, consoleErrors);
    await app.openProject();
    await use(app);
    const inert = await app.inertReports();
    if (inert.length > 0) {
      await testInfo.attach("inline handlers parsed off the page", {
        body: inert.join("\n"),
        contentType: "text/plain",
      });
    }
    // console errors are reported, page errors fail
    for (const line of errors) {
      if (line.startsWith("page:")) problems.push(line);
      else consoleErrors.push(line);
    }
    if (consoleErrors.length > 0) {
      await testInfo.attach("console errors", {
        body: consoleErrors.join("\n"),
        contentType: "text/plain",
      });
    }
  },
});

export { expect };

/** Compares `actual` with the text golden `name` (`--update` writes it instead); a difference is attached. */
export async function expectGolden(name: string, actual: string): Promise<void> {
  const file = path.join(GOLDENS_DIR, name);
  const golden = existsSync(file) ? readFileSync(file, "utf8") : null;
  if (UPDATE) {
    if (golden !== actual) writeFileSync(file, actual);
    return;
  }
  if (golden !== actual) {
    await test.info().attach(`${name} (actual)`, { body: actual, contentType: "text/plain" });
  }
  expect
    .soft(golden ?? `no golden ${name}: record it with --update`, `text golden ${name}`)
    .toBe(actual);
}

/** Findings already known (`known-issues.json`): reported on the test, not failing it, until they are fixed. */
type KnownIssue = { finding: string; why: string };

function knownIssues(): KnownIssue[] {
  const all: unknown = JSON.parse(readFileSync(path.join(DEEP_DIR, "known-issues.json"), "utf8"));
  return Array.isArray(all)
    ? all.filter(
        (k): k is KnownIssue =>
          typeof k === "object" &&
          k !== null &&
          typeof Reflect.get(k, "finding") === "string" &&
          typeof Reflect.get(k, "why") === "string",
      )
    : [];
}

/** Expects no finding but the known ones; each known one seen becomes an annotation `known issue`. */
export function expectClean(found: readonly string[]): void {
  const known = knownIssues();
  const left = found.filter((finding) => {
    const issue = known.find((k) => finding.includes(k.finding));
    if (issue === undefined) return true;
    test.info().annotations.push({ type: "known issue", description: `${finding} (${issue.why})` });
    return false;
  });
  expect(left, "guards").toEqual([]);
}
