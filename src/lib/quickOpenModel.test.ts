import { describe, expect, it } from "vitest";
import { descriptionMatch, searchFiles, type SearchFilesInput } from "./quickOpenModel";

const FILES = [
  "docs/specs/live-feed.md",
  "docs/specs/watcher.md",
  "docs/research/storage.md",
  "plan.md",
  "README.md",
];
const TAGS: Record<string, string[]> = {
  "docs/specs/live-feed.md": ["spec", "feed", "needs-review"],
  "docs/specs/watcher.md": ["spec", "watcher"],
  "docs/research/storage.md": ["research"],
};
const DESCRIPTIONS: Record<string, string> = {
  "docs/specs/live-feed.md": "How the live feed groups watcher events.",
  "docs/research/storage.md": "Where state lives: AppData or the project folder.",
  "plan.md": "Milestones and the order of the work",
};

function input(over: Partial<SearchFilesInput>): SearchFilesInput {
  return {
    text: "",
    tags: [],
    files: FILES,
    tagsOf: (p) => TAGS[p],
    describe: (p) => DESCRIPTIONS[p] ?? "",
    recency: new Map(),
    limit: 50,
    ...over,
  };
}

const paths = (found: { path: string }[]) => found.map((f) => f.path);

describe("descriptionMatch", () => {
  it("needs every word, ignoring case and accents, and gives the matched positions", () => {
    // "live" starts at 8 and "feed" at 13 in "How the live feed groups"
    expect(descriptionMatch("LIVE feed", "How the live feed groups")).toEqual([
      8, 9, 10, 11, 13, 14, 15, 16,
    ]);
    expect(descriptionMatch("café", "the cafe")).toEqual([4, 5, 6, 7]);
  });

  it("is null when a word is missing or there is nothing to match", () => {
    expect(descriptionMatch("live missing", "the live feed")).toBeNull();
    expect(descriptionMatch("", "text")).toBeNull();
    expect(descriptionMatch("live", "")).toBeNull();
  });
});

describe("searchFiles", () => {
  it("ranks by name like Quick open always did", () => {
    const found = searchFiles(input({ text: "watch" }));
    expect(paths(found)[0]).toBe("docs/specs/watcher.md");
    expect(found[0].positions.length).toBeGreaterThan(0);
  });

  it("filters by tags, all of them", () => {
    expect(paths(searchFiles(input({ text: "md", tags: ["spec"] }))).sort()).toEqual([
      "docs/specs/live-feed.md",
      "docs/specs/watcher.md",
    ]);
    expect(paths(searchFiles(input({ text: "md", tags: ["spec", "needs-review"] })))).toEqual([
      "docs/specs/live-feed.md",
    ]);
    expect(searchFiles(input({ text: "md", tags: ["nope"] }))).toEqual([]);
  });

  it("lists the files of a tag-only query, most recent first, then by path", () => {
    const recency = new Map([
      ["docs/specs/watcher.md", 500],
      ["docs/specs/live-feed.md", 100],
    ]);
    expect(paths(searchFiles(input({ tags: ["spec"], recency })))).toEqual([
      "docs/specs/watcher.md",
      "docs/specs/live-feed.md",
    ]);
    expect(paths(searchFiles(input({ tags: ["spec"] })))).toEqual([
      "docs/specs/live-feed.md",
      "docs/specs/watcher.md",
    ]);
  });

  it("finds files by description after the ones found by name", () => {
    const found = searchFiles(input({ text: "milestones" }));
    expect(paths(found)).toEqual(["plan.md"]);
    expect(found[0].positions).toEqual([]);
    expect(found[0].descriptionPositions.length).toBe("milestones".length);

    // live-feed.md has no "watcher" in its name, only in its description
    const both = searchFiles(input({ text: "watcher" }));
    expect(paths(both)).toEqual(["docs/specs/watcher.md", "docs/specs/live-feed.md"]);
    expect(both[1].descriptionPositions.length).toBe("watcher".length);
  });

  it("limits the list", () => {
    expect(searchFiles(input({ text: "md", limit: 2 }))).toHaveLength(2);
    expect(searchFiles(input({ tags: ["spec"], limit: 1 }))).toHaveLength(1);
  });

  it("combines tags with description matches", () => {
    expect(paths(searchFiles(input({ text: "state", tags: ["research"] })))).toEqual([
      "docs/research/storage.md",
    ]);
    expect(searchFiles(input({ text: "state", tags: ["spec"] }))).toEqual([]);
  });
});
