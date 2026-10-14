import { describe, expect, it } from "vitest";
import { crumbOf, flattenResults, groupHits, hitKey, sanitizeSnippet } from "./searchModel";
import type { SearchHit } from "./types";

const hit = (
  path: string,
  score: number,
  startLine = 1,
  headingPath: string[] = [],
): SearchHit => ({
  path,
  score,
  startLine,
  headingPath,
  snippet: "x",
});

describe("groupHits", () => {
  const groups = groupHits([
    hit("docs/world/style-guide.md", 2),
    hit("docs/world/lore.md", 5, 10),
    hit("docs/world/lore.md", 7, 20),
    hit("glossary.md", 3),
  ]);

  it("groups by file, best group first", () => {
    expect(groups.map((g) => g.path)).toEqual([
      "docs/world/lore.md",
      "glossary.md",
      "docs/world/style-guide.md",
    ]);
  });

  it("orders hits inside a group by score and fills name and folder", () => {
    expect(groups[0].hits.map((h) => h.score)).toEqual([7, 5]);
    expect(groups[0].name).toBe("lore.md");
    expect(groups[0].folder).toBe("docs/world");
    expect(groups[1].folder).toBe("");
  });
});

describe("sanitizeSnippet", () => {
  it("keeps mark tags and escapes everything else", () => {
    expect(sanitizeSnippet("a <mark>b</mark> <script>alert(1)</script>")).toBe(
      "a <mark>b</mark> &lt;script>alert(1)&lt;/script>",
    );
  });
  it("leaves existing entities alone", () => {
    expect(sanitizeSnippet("1 &lt; 2 &amp; <mark>3</mark>")).toBe("1 &lt; 2 &amp; <mark>3</mark>");
  });
});

describe("crumbOf", () => {
  it("uses the heading path, or the file name before the first heading", () => {
    expect(crumbOf(hit("a/b.md", 1, 1, ["Characters", "Aria"]))).toEqual(["Characters", "Aria"]);
    expect(crumbOf(hit("a/b.md", 1))).toEqual(["b.md"]);
  });
});

describe("flattenResults", () => {
  const groups = groupHits([hit("a.md", 3, 1), hit("a.md", 2, 5), hit("b.md", 1, 1)]);

  it("lists group rows followed by their hits", () => {
    const rows = flattenResults(groups, new Set());
    expect(rows.map((r) => r.type)).toEqual(["group", "hit", "hit", "group", "hit"]);
  });

  it("skips the hits of collapsed groups", () => {
    const rows = flattenResults(groups, new Set(["a.md"]));
    expect(rows.map((r) => r.type)).toEqual(["group", "group", "hit"]);
  });

  it("builds stable hit keys", () => {
    expect(hitKey(hit("a.md", 1, 5))).toBe("a.md:5");
  });
});

describe("groupHits with worktree copies", () => {
  const groups = groupHits([
    hit("@wt/feature-ui/docs/world/lore.md", 9),
    hit("docs/world/lore.md", 5),
    hit("glossary.md", 2),
    hit("@wt/feature-ui/README.md", 1),
  ]);

  it("lists the project's files before worktree files, each side by score", () => {
    expect(groups.map((g) => g.path)).toEqual([
      "docs/world/lore.md",
      "glossary.md",
      "@wt/feature-ui/docs/world/lore.md",
      "@wt/feature-ui/README.md",
    ]);
  });

  it("labels the location of a worktree copy so the copies can be told apart", () => {
    expect(groups[0].where).toBe("docs/world");
    expect(groups[2].where).toBe("@wt feature-ui / docs/world");
    expect(groups[3].where).toBe("@wt feature-ui");
  });
});
