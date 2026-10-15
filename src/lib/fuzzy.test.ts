import { describe, expect, it } from "vitest";
import { fuzzyMatch, rankPaths } from "./fuzzy";
import { foldChars, highlightSegments } from "./text";

describe("fuzzyMatch", () => {
  it("returns null for an empty query or a non-subsequence", () => {
    expect(fuzzyMatch("", "docs/a.md")).toBeNull();
    expect(fuzzyMatch("zzz", "docs/a.md")).toBeNull();
    expect(fuzzyMatch("md.a", "docs/a.md")).toBeNull();
  });

  it("highlights the matched characters of the file name", () => {
    const m = fuzzyMatch("live", "docs/specs/live-feed.md");
    expect(m?.positions).toEqual([11, 12, 13, 14]);
  });

  it("is case and accent insensitive", () => {
    expect(fuzzyMatch("SAUDE", "docs/saúde.md")).not.toBeNull();
    expect(fuzzyMatch("readme", "README.md")).not.toBeNull();
  });

  it("matches subsequences across the file name", () => {
    const m = fuzzyMatch("lfd", "docs/live-feed.md");
    expect(m).not.toBeNull();
    expect(m?.positions.length).toBe(3);
  });

  it("matches across folders when the name does not contain the query", () => {
    expect(fuzzyMatch("specslive", "docs/specs/live-feed.md")).not.toBeNull();
  });

  it("matches words typed with spaces, each against the path and in any order", () => {
    for (const query of ["token refresh", "refresh token", "  token   refresh ", "token md"]) {
      expect(fuzzyMatch(query, "docs/plans/token-refresh.md"), query).not.toBeNull();
    }
    for (const query of ["auth flow", "flow auth", "auth md"]) {
      expect(fuzzyMatch(query, "docs/auth-flow.md"), query).not.toBeNull();
    }
    // every word has to be there
    expect(fuzzyMatch("token zebra", "docs/plans/token-refresh.md")).toBeNull();
    expect(fuzzyMatch("auth token", "docs/plans/token-refresh.md")).toBeNull();
    // a word can be in a folder name
    expect(fuzzyMatch("plans refresh", "docs/plans/token-refresh.md")).not.toBeNull();
  });

  it("highlights the characters of every word, once", () => {
    const m = fuzzyMatch("refresh token", "docs/token-refresh.md");
    expect(m?.positions).toEqual([5, 6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17]);
  });

  it("reads a space in the query literally first, so a file name with a space is found as before", () => {
    const m = fuzzyMatch("my notes", "docs/my notes.md");
    expect(m?.positions).toEqual([5, 6, 7, 8, 9, 10, 11, 12]);
    expect(m?.score).toBeGreaterThan(900);
  });

  it("scores a one-word query as it always did", () => {
    expect(fuzzyMatch("live", "docs/live.md")?.score).toBe(1000 - "live.md".length * 0.5);
  });
});

describe("rankPaths", () => {
  const paths = [
    "docs/specs/feed-notes.md",
    "docs/live-feed.md",
    "docs/research/watchers.md",
    "live/readme.md",
    "docs/live.md",
  ];

  it("ranks name prefix above name substring above path matches", () => {
    const ranked = rankPaths("live", paths).map((r) => r.path);
    expect(ranked.slice(0, 2).sort()).toEqual(["docs/live-feed.md", "docs/live.md"]);
    expect(ranked[2]).toBe("live/readme.md");
    expect(ranked).not.toContain("docs/research/watchers.md");
  });

  it("prefers the shorter name among prefix matches", () => {
    const ranked = rankPaths("live", paths).map((r) => r.path);
    expect(ranked[0]).toBe("docs/live.md");
  });

  it("breaks ties by recency", () => {
    const tie = ["a/note.md", "b/note.md"];
    const recency = new Map([
      ["b/note.md", 200],
      ["a/note.md", 100],
    ]);
    expect(rankPaths("note", tie, recency).map((r) => r.path)).toEqual(["b/note.md", "a/note.md"]);
  });

  it("finds the file that has all the words, best match first", () => {
    const files = [
      "docs/auth-flow.md",
      "docs/plans/token-refresh.md",
      "docs/auth/token-refresh.md",
      "docs/other.md",
    ];
    const ranked = rankPaths("auth token", files).map((r) => r.path);
    expect(ranked).toEqual(["docs/auth/token-refresh.md"]);
    expect(rankPaths("token refresh", files).map((r) => r.path)).toHaveLength(2);
    expect(rankPaths("refresh token", files).map((r) => r.path)).toHaveLength(2);
  });

  it("limits the result list", () => {
    const many = Array.from({ length: 80 }, (_, i) => `docs/file${i}.md`);
    expect(rankPaths("file", many)).toHaveLength(50);
    expect(rankPaths("file", many, new Map(), 5)).toHaveLength(5);
  });
});

describe("text helpers", () => {
  it("foldChars keeps the length", () => {
    expect(foldChars("Saúde")).toBe("saude");
    expect(foldChars("İstanbul").length).toBe("İstanbul".length);
  });

  it("highlightSegments merges runs", () => {
    expect(highlightSegments("live-feed", [0, 1, 2, 3])).toEqual([
      { text: "live", match: true },
      { text: "-feed", match: false },
    ]);
    expect(highlightSegments("abc", [])).toEqual([{ text: "abc", match: false }]);
  });
});
