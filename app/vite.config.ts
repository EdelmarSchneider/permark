import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "node:path";
import process from "node:process";
import { defineConfig } from "vitest/config";

const host = process.env["TAURI_DEV_HOST"];

// Agent worktrees live under `.claude/` of the folder Vite runs in: not part of this app, never watched.
const toolFolder = `${process.cwd().split(path.sep).join("/")}/.claude/**`;

// `vite --mode mock` (`npm run dev:mock`): browser UI against the mock backend (src/api/mock.ts).
export default defineConfig({
  plugins: [svelte()],

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
  build: { assetsInlineLimit: 0 },

  test: {
    include: ["src/**/*.test.ts", "scripts/**/*.test.ts"],
    environment: "node",
  },
});
