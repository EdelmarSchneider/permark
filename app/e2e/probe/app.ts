// The real app (a PerMark build) for one probe run: started on a project folder with its web views open to the DevTools
// protocol on a free loopback port, or an app already started that way. Whatever starts here is stopped in `stop`
// (teardown symmetry). A debug build keeps its state in the `PerMark-dev` profile, never in the person's.

import { chromium, type Browser, type Page } from "@playwright/test";
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { freePort, kill } from "./server.ts";

export type RealApp = { cdpUrl: string; stop(): void };

const READY_MS = 90_000;
const WAIT = 30_000;

/** The fixture documents at the paths the mock project gives them (`export_mock_documents`, `src-tauri/src/plugins.rs`). */
const FIXTURE_LAYOUT: readonly (readonly [string, string])[] = [
  ["README.md", "README.md"],
  ["docs/guide.md", "guide.md"],
  ["docs/handbook.md", "handbook.md"],
  ["notes/ideas.md", "ideas.md"],
  ["CLAUDE.md", "claude-rules.md"],
  ["Cargo.toml", "cargo.toml.sample"],
  ["docker-compose.yml", "compose.yml.sample"],
  [".env.example", "env.example.sample"],
  [".editorconfig", "editorconfig.sample"],
  ["config/app.properties", "app.properties.sample"],
  [".gitignore", "gitignore.sample"],
  ["data/team.toon", "team.toon.sample"],
];

/** A copy of the fixture documents in a new temp folder, laid out as the mock project; `remove` deletes it. */
export function fixtureProject(appDir: string): { dir: string; remove(): void } {
  const fixtures = path.join(appDir, "src", "api", "fixtures");
  const dir = mkdtempSync(path.join(tmpdir(), "permark-parity-"));
  for (const [at, file] of FIXTURE_LAYOUT) {
    const target = path.join(dir, ...at.split("/"));
    mkdirSync(path.dirname(target), { recursive: true });
    // the bytes the mock documents are rendered from (a Windows checkout may hold CRLF)
    writeFileSync(target, readFileSync(path.join(fixtures, file), "utf8").replace(/\r\n/g, "\n"));
  }
  return {
    dir,
    remove: () => rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }),
  };
}

async function ready(cdpUrl: string, exited: () => number | null): Promise<void> {
  const until = Date.now() + READY_MS;
  while (Date.now() < until) {
    const code = exited();
    if (code !== null) {
      // a running PerMark of the same build kind takes the folder and this one exits
      throw new Error(
        `app exited (${code}) before its web view opened: is a PerMark of this build already running? close it or use --cdp`,
      );
    }
    try {
      const res = await fetch(`${cdpUrl}/json/version`);
      if (res.ok) return;
    } catch {
      // not listening yet
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`app's web view not reachable in ${READY_MS / 1000} s`);
}

/** Starts `exe` on `project` with remote debugging on a free loopback port. */
export async function startApp(exe: string, project: string): Promise<RealApp> {
  const port = await freePort();
  const own = `--remote-debugging-port=${port}`;
  const extra = process.env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"];
  const child = spawn(exe, [project], {
    stdio: "ignore",
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: extra ? `${extra} ${own}` : own,
    },
  });
  let failed: Error | null = null;
  child.once("error", (e) => (failed = e));
  const cdpUrl = `http://127.0.0.1:${port}`;
  const stop = (): void => kill(child);
  try {
    await ready(cdpUrl, () => {
      if (failed !== null) throw failed;
      return child.exitCode;
    });
  } catch (e) {
    stop();
    throw e;
  }
  return { cdpUrl, stop };
}

type PageFacts = { tree: boolean; visible: boolean; title: string };

/** The app's view on screen that shows a project tree (a window holds one view per tab). */
async function projectPage(browser: Browser): Promise<Page> {
  const until = Date.now() + WAIT;
  for (;;) {
    for (const page of browser.contexts().flatMap((c) => c.pages())) {
      const facts = await page
        .evaluate((): PageFacts => ({
          tree: document.querySelector('[role="treeitem"]') !== null,
          visible: document.visibilityState === "visible",
          title: document.title,
        }))
        .catch((): PageFacts | null => null);
      if (facts?.tree === true && facts.visible) return page;
    }
    if (Date.now() > until) throw new Error("no view of the app shows a project tree");
    await new Promise((r) => setTimeout(r, 250));
  }
}

/**
 * Sets the app's theme setting as the person's choice in Settings does (`system` | `light` | `dark`) and waits until
 * the page shows `shown`; resolves to the setting before.
 */
export async function setTheme(page: Page, theme: string, shown: string | null): Promise<string> {
  const before = await page.evaluate(async (value) => {
    const internals: unknown = Reflect.get(window, "__TAURI_INTERNALS__");
    const invoke: unknown =
      typeof internals === "object" && internals !== null ? Reflect.get(internals, "invoke") : null;
    if (typeof invoke !== "function") throw new Error("not the app's page (no IPC)");
    const call = (cmd: string, args: object): Promise<unknown> =>
      Promise.resolve(Reflect.apply(invoke, internals, [cmd, args]));
    const settings = await call("settings_get", {});
    const old: unknown =
      typeof settings === "object" && settings !== null ? Reflect.get(settings, "theme") : null;
    await call("settings_set", { change: { key: "theme", value } });
    return typeof old === "string" ? old : "system";
  }, theme);
  if (shown !== null) {
    await page.waitForFunction((t) => document.documentElement.dataset["theme"] === t, shown, {
      timeout: WAIT,
    });
  }
  return before;
}

/** The app at `cdpUrl`: its project view, console | page errors going to `errors`; `close` only disconnects. */
export async function connectApp(
  cdpUrl: string,
  errors: string[],
): Promise<{ page: Page; close(): Promise<void> }> {
  const browser = await chromium.connectOverCDP(cdpUrl);
  try {
    const page = await projectPage(browser);
    page.on("console", (msg) => {
      if (msg.type() === "error") errors.push(`console: ${msg.text().slice(0, 300)}`);
    });
    page.on("pageerror", (err) => errors.push(`page: ${String(err).slice(0, 300)}`));
    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    return { page, close: () => browser.close() };
  } catch (e) {
    await browser.close();
    throw e;
  }
}
