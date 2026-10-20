import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "node:path";
import process from "node:process";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

// KaTeX lists woff2, woff and ttf for every font; the webviews PerMark runs on all read woff2, so only that ships.
function katexWoff2Only(): Plugin {
  return {
    name: "katex-woff2-only",
    enforce: "pre",
    transform(code, id) {
      if (!id.includes("katex") || !id.split("?")[0]?.endsWith(".css")) return null;
      return code.replace(
        /,url\([^)]*\.woff\) format\("woff"\),url\([^)]*\.ttf\) format\("truetype"\)/g,
        "",
      );
    },
  };
}

const host = process.env["TAURI_DEV_HOST"];

// Agent worktrees live under `.claude/` of the folder Vite runs in: not part of this app, never watched.
const toolFolder = `${process.cwd().split(path.sep).join("/")}/.claude/**`;

// `vite --mode mock` (`npm run dev:mock`): browser UI against the mock backend (src/api/mock.ts).
export default defineConfig({
  plugins: [svelte(), katexWoff2Only()],

  // component tests mount the browser build of Svelte (jsdom), not the server one
  resolve: process.env["VITEST"] ? { conditions: ["browser"] } : undefined,

  // Tauri: keep Rust errors visible; fixed port (= tauri.conf.json devUrl).
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**", toolFolder] },
  },

  // Fonts + icons ship as files (CSP font-src 'self'; no data: URLs).
  // The diagram engines (PlantUML ~3 MB, Mermaid's layout ~1.5 MB) are chunks of their own, loaded only when a
  // diagram comes near the screen: the size warning is about the main bundle, which stays small.
  // The build prints how long plugin callbacks took when they are a tenth of it (the CSS transform of this many
  // components is): a timing, not a problem.
  build: {
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 3500,
    rolldownOptions: { checks: { pluginTimings: false } },
  },

  test: {
    include: ["src/**/*.test.ts", "scripts/**/*.test.ts"],
    environment: "node",
    // under a full parallel run (CI, a busy machine) a component test's first mount + lazy imports can pass the 5 s
    // default; the tests' own waits (10 s) and time bounds decide, the runner's limit must not cut in first
    testTimeout: 30_000,
  },
});
