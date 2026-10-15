import { describe, expect, it } from "vitest";
import config from "../../vite.config";

describe("the dev server's watcher", () => {
  const resolved =
    typeof config === "function" ? config({ command: "serve", mode: "development" }) : config;

  it("leaves out src-tauri and the worktrees under .claude", async () => {
    const ignored = (await resolved).server?.watch?.ignored;
    const root = process.cwd().replace(/\\/g, "/");
    expect(ignored).toEqual(["**/src-tauri/**", `${root}/.claude/**`]);
  });

  it("does not leave out every file of a dev server that runs inside such a worktree", async () => {
    // a pattern like **/.claude/** would match the whole project then, because the project's own path has .claude in it
    const ignored = (await resolved).server?.watch?.ignored as string[];
    expect(ignored).not.toContain("**/.claude/**");
  });
});
