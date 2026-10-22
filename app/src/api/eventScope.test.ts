// Every backend event reaches a view through `api` (src/api/tauri.ts), which listens in this webview only. A file that
// listens by itself (`@tauri-apps/api/event`, a window's `listen`) hears every view's events again (Tauri's Any target).
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(import.meta.dirname, "..");

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "generated" ? [] : sources(path);
    return /\.(ts|svelte)$/.test(entry.name) && !/\.test\.ts$/.test(entry.name) ? [path] : [];
  });
}

describe("event scope", () => {
  it("no file outside the api listens to backend events by itself", () => {
    const own =
      /@tauri-apps\/api\/event|getCurrentWindow\(\)\s*\.\s*listen|getCurrentWebview\(\)\s*\.\s*listen/;
    const offenders = sources(SRC)
      .map((path) => relative(SRC, path).split(sep).join("/"))
      .filter((path) => path !== "api/tauri.ts")
      .filter((path) => own.test(readFileSync(join(SRC, path), "utf8")));
    expect(offenders).toEqual([]);
  });
});
