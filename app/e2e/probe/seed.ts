// Seeds: the mock app's state for a scenario (spec: ux-probe.md §6). They reach the app's OWN mock backend through the dev
// server (`/src/api/mock.ts` is the module instance the app uses), or drive the UI where the app has a stable route
// (tree rows by `data-path`). Fixture documents only.

import type { Page } from "@playwright/test";
import { observeRead, observeStart, observeStop } from "./page.ts";
import type { SeedUse } from "./scenario.ts";

const WAIT = 15_000;

/** Waits until the page has been quiet for `quietMs` (or `maxMs` passed). */
export async function settle(page: Page, quietMs = 250, maxMs = 5_000): Promise<void> {
  await page.evaluate(observeStart, null);
  const start = Date.now();
  try {
    for (;;) {
      await page.waitForTimeout(50);
      const elapsed = Date.now() - start;
      const seen = await page.evaluate(observeRead);
      const last = seen.lastAtMs ?? 0;
      if (elapsed - last >= quietMs || elapsed >= maxMs) return;
    }
  } finally {
    await page.evaluate(observeStop);
  }
}

/** Runs `fn` in the page with the app's mock backend (`mockApi`) and the mock module's exports. */
async function withMock<A, R>(
  page: Page,
  fn: (m: { mockApi: MockApi; MOCK_FOLDER: string }, arg: A) => R | Promise<R>,
  arg: A,
): Promise<R> {
  return page.evaluate(
    async ({ source, arg }) => {
      const mockUrl = "/src/api/mock.ts";
      const apiUrl = "/src/api/index.ts";
      const m = (await import(mockUrl)) as { mockApi: unknown; MOCK_FOLDER: string };
      const app = (await import(apiUrl)) as { api: unknown };
      if (app.api !== m.mockApi)
        throw new Error("mock module not shared (is the server in mock mode?)");
      const run = new Function(`return (${source})`)() as (m: unknown, arg: unknown) => unknown;
      return run(m, arg);
    },
    { source: fn.toString(), arg },
  ) as Promise<R>;
}

/** The parts of the mock backend the seeds use (shapes from `src/api/mock.ts`). */
type MockApi = {
  openPath(path: string, placement: string): Promise<unknown>;
  openNotes(placement: string): Promise<unknown>;
  settingsSet(change: { key: string; value: unknown }): Promise<unknown>;
  planImport(
    target: string,
    sources: string[],
  ): Promise<{ ok: boolean; value?: { planId: number } }>;
  applyImport(planId: number, mode: string, conflicts: string): Promise<{ ok: boolean }>;
  emit(event: string, payload: unknown): void;
  listWindows(): Promise<{
    ok: boolean;
    value?: { tabs: { tab: number }[]; active: number | null }[];
  }>;
  /** The system's drag events for the view (`api.onDrop`), as Tauri's would arrive (CSS pixels). */
  pluginCall: (method: string, args: Record<string, unknown>) => Promise<unknown>;
  probe: {
    project: { add(path: string, text: string): void; files: Map<string, string> };
    drops: { fire(event: unknown): void };
  };
};

function str(args: Record<string, unknown>, key: string, def?: string): string {
  const v = args[key] ?? def;
  if (typeof v !== "string" || v === "") throw new Error(`seed: "${key}" text expected`);
  return v;
}

const rowOf = (page: Page, path: string) =>
  page.locator(`[role="treeitem"][data-path="${path.replace(/"/g, '\\"')}"]`);

/** One system drag event (`over` | `drop`) for the view, at a point in CSS pixels. */
export async function fireDrop(
  page: Page,
  event:
    | { type: "over"; count: number; paths: string[]; x: number; y: number }
    | { type: "drop"; paths: string[]; x: number; y: number },
): Promise<void> {
  await withMock(page, (m, e) => m.mockApi.probe.drops.fire(e), event);
}

/** Applies one seed. */
export async function seed(page: Page, use: SeedUse, args: Record<string, unknown>): Promise<void> {
  switch (use) {
    case "project": {
      const path = typeof args["path"] === "string" ? args["path"] : null;
      await withMock(
        page,
        async (m, p) => void (await m.mockApi.openPath(p ?? m.MOCK_FOLDER, "tab")),
        path,
      );
      await page.locator('[role="treeitem"]').first().waitFor({ state: "visible", timeout: WAIT });
      break;
    }
    case "tab": {
      const before = await page.locator("[data-tab]").count();
      await withMock(
        page,
        async (m, p) => void (await m.mockApi.openPath(p, "tab")),
        str(args, "path"),
      );
      await page.waitForFunction(
        (n) => document.querySelectorAll("[data-tab]").length > n,
        before,
        {
          timeout: WAIT,
        },
      );
      break;
    }
    case "notes":
      await withMock(page, async (m) => void (await m.mockApi.openNotes("tab")), null);
      break;
    case "doc": {
      const path = str(args, "path");
      const parts = path.split("/");
      for (let i = 1; i < parts.length; i++) {
        const row = rowOf(page, parts.slice(0, i).join("/"));
        await row.waitFor({ state: "visible", timeout: WAIT });
        if ((await row.getAttribute("aria-expanded")) === "false")
          await row.locator(".chev").click();
      }
      const row = rowOf(page, path);
      await row.waitFor({ state: "visible", timeout: WAIT });
      await row.click();
      const name = parts[parts.length - 1] ?? path;
      await page.waitForFunction(
        (n) => document.querySelector(".doc-header")?.textContent?.includes(n) === true,
        name,
        { timeout: WAIT },
      );
      break;
    }
    case "file":
      await withMock(page, (m, a) => m.mockApi.probe.project.add(a.path, a.text), {
        path: str(args, "path"),
        text: typeof args["text"] === "string" ? args["text"] : "# New\n",
      });
      break;
    case "setting":
      await withMock(page, async (m, a) => void (await m.mockApi.settingsSet(a)), {
        key: str(args, "key"),
        value: args["value"],
      });
      break;
    case "rightPanel": {
      // `aside.panel` (older builds: no data-region, no hide state) | `aside[data-region="right"]`
      const open = args["open"] !== false;
      const shown = (): Promise<boolean | null> =>
        page.evaluate(() => {
          const el = document.querySelector('aside[data-region="right"], aside.panel');
          if (el === null) return null;
          return !el.classList.contains("gone") && el.getBoundingClientRect().width > 0;
        });
      const now = await shown();
      if (now === null) throw new Error("seed rightPanel: no right panel in this view");
      if (now !== open) {
        await page.keyboard.press("Control+Alt+b");
        const until = Date.now() + WAIT;
        while ((await shown()) !== open) {
          if (Date.now() > until)
            throw new Error(`seed rightPanel: panel not ${open ? "shown" : "hidden"}`);
          await page.waitForTimeout(100);
        }
      }
      break;
    }
    case "foreignTreeNews": {
      const count = typeof args["count"] === "number" ? args["count"] : 50;
      await withMock(
        page,
        (m, n) => {
          const files = m.mockApi.probe.project.files.size;
          for (let seq = 1; seq <= n; seq++) {
            m.mockApi.emit("tree-changed", {
              window: "w-9",
              delta: {
                seq,
                removed: [],
                added: [],
                renamed: [],
                totalFiles: files,
                shownFiles: files,
                bulk: false,
                filter: null,
              },
            });
          }
        },
        count,
      );
      break;
    }
    case "failPlugin": {
      const error = args["error"];
      const failure =
        typeof error === "object" && error !== null
          ? (error as { kind?: unknown; subject?: unknown })
          : { kind: "unavailable", subject: null };
      await withMock(
        page,
        (m, a) => {
          const original = m.mockApi.pluginCall;
          m.mockApi.pluginCall = (method, callArgs) => {
            const matches =
              method === a.method &&
              Object.entries(a.match).every(
                ([k, v]) => JSON.stringify(callArgs[k]) === JSON.stringify(v),
              );
            if (!matches) return original(method, callArgs);
            return Promise.resolve({
              ok: false,
              error: {
                kind: "backend",
                command: "plugin_call",
                error: { kind: a.kind, subject: a.subject, detail: null },
              },
            });
          };
        },
        {
          method: str(args, "method"),
          match: (typeof args["match"] === "object" && args["match"] !== null
            ? args["match"]
            : {}) as Record<string, unknown>,
          kind: typeof failure.kind === "string" ? failure.kind : "unavailable",
          subject: typeof failure.subject === "string" ? failure.subject : null,
        },
      );
      break;
    }
    case "importFiles": {
      const sources = args["sources"];
      if (!Array.isArray(sources) || !sources.every((s) => typeof s === "string")) {
        throw new Error('seed importFiles: "sources" text list expected');
      }
      await withMock(
        page,
        async (m, a) => {
          const plan = await m.mockApi.planImport(a.target, a.sources);
          if (!plan.ok || !plan.value) throw new Error("planImport failed");
          const done = await m.mockApi.applyImport(plan.value.planId, "copy", "keepBoth");
          if (!done.ok) throw new Error("applyImport failed");
        },
        {
          target: typeof args["target"] === "string" ? args["target"] : "",
          sources: sources as string[],
        },
      );
      break;
    }
    case "tabShown": {
      // what the backend tells a view whose tab another one replaced on screen (`other`: hidden, state kept) | that
      // comes back (`this`); the mock's page stays this view (one page for all tabs)
      const which = str(args, "tab");
      if (which !== "other" && which !== "this")
        throw new Error('seed tabShown: "tab" other | this expected');
      await withMock(
        page,
        async (m, w) => {
          const listed = await m.mockApi.listWindows();
          const own = listed.ok && listed.value ? listed.value[0] : undefined;
          if (!listed.value || !own) throw new Error("seed tabShown: no window");
          const other = own.tabs.find((t) => t.tab !== own.active);
          if (w === "other" && !other)
            throw new Error("seed tabShown: needs a second tab (seed tab)");
          const active = w === "other" && other ? other.tab : own.active;
          m.mockApi.emit("windows-changed", {
            windows: [{ ...own, active }, ...listed.value.slice(1)],
          });
        },
        which,
      );
      break;
    }
    default:
      return use satisfies never;
  }
}
