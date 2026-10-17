import { describe, expect, it } from "vitest";
import {
  countMatches,
  descriptionMatch,
  onMoreRow,
  quickMatches,
  searchFiles,
  searchFolders,
  withAssets,
  type QuickMatchesInput,
  type SearchFilesInput,
} from "./quickOpenModel";

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

describe("searchFiles with several words", () => {
  it("finds a file whose path has every word, in any order", () => {
    for (const text of ["live feed", "feed live", "specs feed", "feed md"]) {
      expect(paths(searchFiles(input({ text }))), text).toContain("docs/specs/live-feed.md");
    }
    expect(paths(searchFiles(input({ text: "live zebra" })))).toEqual([]);
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

describe("countMatches (the hidden files that match)", () => {
  const HIDDEN = ["src/live.rs", "src/watcher.rs", "assets/logo.png", "docs/archive/live-notes.md"];
  const hidden = (over: Partial<SearchFilesInput>) => input({ files: HIDDEN, ...over });

  it("counts every hidden file whose path matches the words, not just the first page", () => {
    expect(countMatches(hidden({ text: "live" }))).toBe(2);
    expect(countMatches(hidden({ text: "watch" }))).toBe(1);
    expect(countMatches(hidden({ text: "zzz" }))).toBe(0);
    const many = Array.from({ length: 80 }, (_, i) => `gen/file-${i}.rs`);
    expect(countMatches(input({ files: many, text: "file", limit: 50 }))).toBe(80);
  });

  it("matches descriptions and tags like the visible list does", () => {
    const withMeta = hidden({
      text: "milestones",
      describe: (p) => (p === "src/live.rs" ? "Milestones of the live feed" : ""),
    });
    expect(countMatches(withMeta)).toBe(1);
    const tagged = hidden({
      tags: ["spec"],
      tagsOf: (p) => (p === "src/watcher.rs" ? ["spec"] : []),
    });
    expect(countMatches(tagged)).toBe(1);
  });

  it("is zero when nothing is typed or nothing is hidden", () => {
    expect(countMatches(hidden({ text: "  " }))).toBe(0);
    expect(countMatches(input({ files: [], text: "live" }))).toBe(0);
  });
});

describe("quickMatches (what Quick open lists when the Explorer filter hides files)", () => {
  const SHOWN = ["docs/plan.md", "docs/live-feed.md", "README.md"];
  const HIDDEN = [".permark/notes.md", "docs/archive/live-notes.md"];
  const query = (over: Partial<QuickMatchesInput>): QuickMatchesInput => ({
    ...input({ files: SHOWN }),
    hiddenFiles: HIDDEN,
    allFiles: [...SHOWN, ...HIDDEN],
    reveal: false,
    ...over,
  });

  it("lists the visible matches and counts the hidden ones behind a row of their own", () => {
    const m = quickMatches(query({ text: "live" }));
    expect(paths(m.found)).toEqual(["docs/live-feed.md"]);
    expect(m.found.some((f) => f.hidden)).toBe(false);
    expect(m.moreHidden).toBe(1);
    // the first row is a file: Enter opens it, it does not reveal anything
    expect(onMoreRow(0, m.found.length, m.moreHidden)).toBe(false);
    // the person arrows down to the row of the hidden matches
    expect(onMoreRow(1, m.found.length, m.moreHidden)).toBe(true);
  });

  it("lists the hidden matches at once when nothing visible matches, so one Enter opens the first", () => {
    const m = quickMatches(query({ text: "notes" }));
    expect(paths(m.found)).toEqual([".permark/notes.md", "docs/archive/live-notes.md"]);
    expect(m.found.every((f) => f.hidden)).toBe(true);
    expect(m.moreHidden).toBe(0);
    // the selected row (the first) is a file, not the row that only reveals
    expect(onMoreRow(0, m.found.length, m.moreHidden)).toBe(false);
  });

  it("does the same for a dot-path found by a tag, and for a description match", () => {
    const tagged = quickMatches(
      query({ tags: ["wip"], tagsOf: (p) => (p === ".permark/notes.md" ? ["wip"] : []) }),
    );
    expect(paths(tagged.found)).toEqual([".permark/notes.md"]);
    expect(tagged.found[0].hidden).toBe(true);
    expect(tagged.moreHidden).toBe(0);

    const described = quickMatches(
      query({
        text: "retrospective",
        describe: (p) => (p === "docs/archive/live-notes.md" ? "Sprint retrospective" : ""),
      }),
    );
    expect(paths(described.found)).toEqual(["docs/archive/live-notes.md"]);
    expect(described.found[0].hidden).toBe(true);
  });

  it("lists both when the hidden matches were revealed, the hidden ones marked", () => {
    const m = quickMatches(query({ text: "live", reveal: true }));
    expect(paths(m.found).sort()).toEqual(["docs/archive/live-notes.md", "docs/live-feed.md"]);
    expect(m.found.find((f) => f.path === "docs/archive/live-notes.md")?.hidden).toBe(true);
    expect(m.found.find((f) => f.path === "docs/live-feed.md")?.hidden).toBe(false);
    expect(m.moreHidden).toBe(0);
  });

  it("has nothing to list or to reveal when no file matches", () => {
    const m = quickMatches(query({ text: "zzz" }));
    expect(m).toEqual({ found: [], moreHidden: 0 });
    expect(onMoreRow(0, 0, 0)).toBe(false);
  });

  it("has no hidden files to offer when the filter hides none", () => {
    const m = quickMatches(query({ text: "live", hiddenFiles: [], allFiles: SHOWN }));
    expect(paths(m.found)).toEqual(["docs/live-feed.md"]);
    expect(m.moreHidden).toBe(0);
  });
});

describe("withAssets", () => {
  it("lists pictures, sound and video the filter hides, and keeps the rest hidden", () => {
    const listed = withAssets(
      ["README.md", "docs/a.md"],
      [
        "src/main.rs",
        "src-tauri/icons/icon.png",
        "build/demo.mp4",
        "sounds/ping.WAV",
        "Cargo.lock",
      ],
    );
    expect(listed.files).toEqual([
      "README.md",
      "docs/a.md",
      "src-tauri/icons/icon.png",
      "build/demo.mp4",
      "sounds/ping.WAV",
    ]);
    expect(listed.hidden).toEqual(["src/main.rs", "Cargo.lock"]);
  });

  it("keeps what is in a hidden folder hidden, pictures included", () => {
    const listed = withAssets(
      [],
      [".github/social.png", "docs/.cache/clip.mp4", "assets/logo.png"],
    );
    expect(listed.files).toEqual(["assets/logo.png"]);
    expect(listed.hidden).toEqual([".github/social.png", "docs/.cache/clip.mp4"]);
  });

  it("finds a hidden picture by its name without the +N hidden files row", () => {
    const { files, hidden } = withAssets(
      ["docs/plan.md", "README.md"],
      ["tools/diagram.png", "tools/build.rs"],
    );
    const m = quickMatches({
      ...input({ files, text: "diagram" }),
      hiddenFiles: hidden,
      allFiles: [...files, ...hidden],
      reveal: false,
    });
    expect(paths(m.found)).toEqual(["tools/diagram.png"]);
    expect(m.found[0].hidden).toBe(false);
    expect(m.moreHidden).toBe(0);
  });
});

describe("searchFolders", () => {
  const FOLDERS = ["docs", "docs/design", "docs/design/assets", "src", "src/media", "@wt/feature"];

  it("lists nothing until something is typed", () => {
    expect(searchFolders("", FOLDERS, new Map(), 10)).toEqual([]);
    expect(searchFolders("   ", FOLDERS, new Map(), 10)).toEqual([]);
  });

  it("finds the folders whose path has the words, with the matched characters", () => {
    const found = searchFolders("design", FOLDERS, new Map(), 10);
    expect(found.map((f) => f.path)).toEqual(["docs/design", "docs/design/assets"]);
    expect(found[0].positions.length).toBeGreaterThan(0);
    expect(searchFolders("feature", FOLDERS, new Map(), 10).map((f) => f.path)).toEqual([
      "@wt/feature",
    ]);
    expect(searchFolders("zzz", FOLDERS, new Map(), 10)).toEqual([]);
  });

  it("puts the folder used lately first among equals, and stops at the limit", () => {
    const recency = new Map([["docs/design/assets", 5]]);
    expect(searchFolders("as", FOLDERS, recency, 10)[0].path).toBe("docs/design/assets");
    expect(searchFolders("d", FOLDERS, new Map(), 2)).toHaveLength(2);
  });
});
