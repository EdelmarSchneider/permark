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
