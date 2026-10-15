import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "node:path";
import process from "node:process";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

// Tooling keeps checkouts of this project under `.claude/` (git worktrees): a change there is not a change of this app,
// and Vite would reload the page for it. The pattern starts at the folder Vite runs in, so that a dev server started
// inside such a checkout still watches its own files.
const toolFolder = `${process.cwd().split(path.sep).join("/")}/.claude/**`;

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  plugins: [svelte()],

  // `vite --mode mock` runs the UI in a browser against mockApi (same as VITE_MOCK=1)
  define: mode === "mock" ? { "import.meta.env.VITE_MOCK": JSON.stringify("1") } : {},

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri` and the checkouts under `.claude/`
      ignored: ["**/src-tauri/**", toolFolder],
    },
  },

  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    setupFiles: ["src/test/setup.ts"],
  },
}));
