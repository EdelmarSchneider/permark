import { describe, expect, it } from "vitest";
import { isQuietPath, withoutQuiet, type Interest } from "./quiet";
import type { FsBatch } from "./types";

function marks(pairs: [string, Interest][]): Map<string, Interest> {
  return new Map(pairs);
}

describe("isQuietPath", () => {
  it("lets everything speak until a mark says otherwise", () => {
    expect(isQuietPath("src/main.rs", new Map())).toBe(false);
    expect(isQuietPath("", new Map())).toBe(false);
  });

  it("covers a folder, and a closer mark wins", () => {
    const book = marks([
      ["src", "quiet"],
      ["src/docs", "speaking"],
      ["src/docs/drafts", "quiet"],
    ]);
    expect(isQuietPath("src", book)).toBe(true);
    expect(isQuietPath("src/main.rs", book)).toBe(true);
    expect(isQuietPath("src/docs", book)).toBe(false);
    expect(isQuietPath("src/docs/guide.md", book)).toBe(false);
    expect(isQuietPath("src/docs/drafts/old.md", book)).toBe(true);
    expect(isQuietPath("README.md", book)).toBe(false);
  });

  it("does not let the project root silence a worktree", () => {
    const book = marks([["", "quiet"]]);
    expect(isQuietPath("src/main.rs", book)).toBe(true);
    expect(isQuietPath("@wt/feature/src/main.rs", book)).toBe(false);
  });
});

describe("withoutQuiet", () => {
  const batch = (paths: string[]): FsBatch => ({
    changes: paths.map((path) => ({ kind: "modified", path, atMs: 1 })),
    bulk: null,
    rescan: false,
  });

  it("drops a change in a silenced folder and keeps one that speaks inside it", () => {
    const quiet = (path: string) => path === "src" || path.startsWith("src/") && !path.startsWith("src/docs");
    const out = withoutQuiet(batch(["src/main.rs", "src/docs/guide.md", "README.md"]), quiet);
    expect(out.changes.map((change) => change.path)).toEqual(["src/docs/guide.md", "README.md"]);
  });
});
